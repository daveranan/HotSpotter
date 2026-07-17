//! Stage 8B bounded deterministic registered texture quilting.

use std::collections::BTreeMap;
use hot_trimmer_domain::{AlgorithmProvenance, ContentDigest, MaterialBehaviorClass, MaterialChannelRole,
    ScaleProvenance, StageResult};
use hot_trimmer_render_core::{PreparedExemplarChannel, RenderCancellationToken};
use super::*;

pub const STAGE_08B_QUILTING_ALGORITHM_ID: &str = "hot_trimmer.registered_texture_quilting";
pub const STAGE_08B_ALGORITHM_VERSION: &str = "8.2.0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuiltingPatchSize {
    RelativeMilli { width: u16, height: u16 },
    /// Uses only the world-accurate, provenance-bearing Stage 6 scale report.
    PhysicalMicrometers { width: u32, height: u32 },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum BandAxis { Horizontal, Vertical }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuiltingSemanticConstraint {
    StochasticIsotropic,
    Directional { behavior: MaterialBehaviorClass,
        requested_angle_millidegrees: i32, tolerance_millidegrees: u32 },
    PeriodAlignedLattice { period_x: u16, period_y: u16, allow_period_aligned_quilting: bool },
    Banded { axis: BandAxis, period_pixels: u16 },
    UniqueDetail, ManufacturedPattern, MixedUnknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuiltingWeights { pub overlap: u16, pub histogram: u16, pub structure: u16,
    pub duplicate_use: u16, pub boundary_periodicity: u16 }
impl Default for QuiltingWeights { fn default() -> Self { Self { overlap: 420, histogram: 130,
    structure: 170, duplicate_use: 190, boundary_periodicity: 90 } } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuiltingSettings {
    pub output_width: u32, pub output_height: u32, pub patch_size: QuiltingPatchSize,
    pub overlap_milli: u16, pub pyramid_levels: u8, pub candidate_count: u16,
    pub near_best_count: u16, pub near_best_threshold_milli: u16,
    pub minimum_usable_confidence_milli: u16, pub weights: QuiltingWeights,
    pub max_accepted_seam_cost_milli: u16,
    pub max_boundary_periodicity_error_milli: u16,
    pub semantics: QuiltingSemanticConstraint, pub max_output_dimension: u32,
    pub max_output_pixels: u64, pub max_patch_count: u32, pub max_candidate_count: u16,
    pub max_working_bytes: u64, pub max_iterations: u32, pub max_operations: u64,
}
impl Default for QuiltingSettings { fn default() -> Self { Self {
    output_width: 1024, output_height: 1024,
    patch_size: QuiltingPatchSize::RelativeMilli { width: 400, height: 400 },
    overlap_milli: 250, pyramid_levels: 3, candidate_count: 48, near_best_count: 6,
    near_best_threshold_milli: 120, minimum_usable_confidence_milli: 650,
    max_accepted_seam_cost_milli: 500, max_boundary_periodicity_error_milli: 500,
    weights: QuiltingWeights::default(), semantics: QuiltingSemanticConstraint::StochasticIsotropic,
    max_output_dimension: 16_384, max_output_pixels: 67_108_864, max_patch_count: 16_384,
    max_candidate_count: 512, max_working_bytes: 2_147_483_648, max_iterations: 16_384,
    max_operations: 2_000_000_000,
} } }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcePatchUsage { pub source_x: u32, pub source_y: u32, pub use_count: u32 }
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuiltPlacement { pub output_x: u32, pub output_y: u32, pub width: u16, pub height: u16,
    pub source_x: u32, pub source_y: u32, pub candidate_rank: u16, pub total_cost_milli: u32 }
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuiltOverlapSeam { pub patch_index: u32, pub axis: SeamAxis, pub output_origin: (u32,u32),
    pub positions: Vec<u16>, pub normalized_energy_milli: u16 }
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuiltingDiagnostics {
    pub patch_size_pixels: (u16,u16), pub overlap_pixels: (u16,u16),
    pub placements: Vec<QuiltPlacement>, pub source_usage: Vec<SourcePatchUsage>,
    pub overlap_seams: Vec<QuiltOverlapSeam>, pub rejected_unusable_candidates: u32,
    pub duplicate_patch_uses: u32, pub mean_seam_energy_milli: u16,
    pub correspondence_confidence_milli: u16, pub boundary_periodicity_error_milli: (u16,u16),
    pub failure_reasons: Vec<String>,
}
#[derive(Clone, Copy)] struct Rect { x:u32,y:u32,w:u32,h:u32 }
#[derive(Clone, Copy)] struct Candidate { x:u32,y:u32,cost:f64 }
#[derive(Clone, Copy, Default)]
struct ScorePixel { rgb:[f64;3], gradient:f64, height:f64, roughness:f64, normal:[f64;3], structure:f64 }
struct ScoreLevel { width:u32, height:u32, pixels:Vec<ScorePixel> }
struct RegisteredScorePyramid { levels:Vec<ScoreLevel> }

pub(super) fn quilted_domain(r:&DomainRequest, key:ContentDigest, cancel:&RenderCancellationToken)
    -> Result<PreparedMaterialDomain,DomainError> {
    let s=r.quilting; let (pw,ph)=patch_size(r)?;
    let ox=((u64::from(pw)*u64::from(s.overlap_milli)/1000).max(2)) as u32;
    let oy=((u64::from(ph)*u64::from(s.overlap_milli)/1000).max(2)) as u32;
    validate(r,pw,ph,ox,oy)?; preflight(r,pw,ph)?; check_cancel(cancel)?;
    let pyramid=build_registered_score_pyramid(r,cancel)?;
    let rects=rects(r,pw,ph,ox,oy)?; let count=pixel_count(s.output_width,s.output_height)?;
    let mut map=vec![None;count]; let mut owner=vec![0;count]; let mut uses=BTreeMap::new();
    let mut placements=Vec::new(); let mut seams=Vec::new(); let mut rejected=0; let mut ops=0;
    for (pi,rect) in rects.iter().copied().enumerate() {
        if pi as u32>=s.max_iterations{return Err(DomainError::QuiltingCoverageFailed)} check_cancel(cancel)?;
        let cs=candidates(r,&pyramid,rect,pw,ph,&map,&uses,&mut rejected,&mut ops,cancel)?;
        let rank=near_best(&cs,r.seed,pi as u64,s); let c=cs[rank]; let src=SourceCoordinate{x:c.x,y:c.y};
        let vs=if rect.x>0{Some(solve_seam(r,&pyramid,rect,src,SeamAxis::X,ox.min(rect.w),&map,pi as u32,cancel)?)}else{None};
        let hs=if rect.y>0{Some(solve_seam(r,&pyramid,rect,src,SeamAxis::Y,oy.min(rect.h),&map,pi as u32,cancel)?)}else{None};
        for seam in [vs.as_ref(),hs.as_ref()].into_iter().flatten(){if seam.normalized_energy_milli>s.max_accepted_seam_cost_milli{
            return Err(DomainError::UnacceptableQuiltingSeam{patch_index:pi as u32,axis:seam.axis,
                cost_milli:seam.normalized_energy_milli,maximum_milli:s.max_accepted_seam_cost_milli})}}
        if let Some(v)=&vs{seams.push(v.clone())} if let Some(h)=&hs{seams.push(h.clone())}
        apply(rect,src,pi as u32,vs.as_ref(),hs.as_ref(),s.output_width,&mut map,&mut owner);
        *uses.entry((src.x,src.y)).or_insert(0)+=1;
        placements.push(QuiltPlacement{output_x:rect.x,output_y:rect.y,width:rect.w as u16,height:rect.h as u16,
            source_x:src.x,source_y:src.y,candidate_rank:rank as u16,total_cost_milli:(c.cost.clamp(0.0,65.535)*1000.0) as u32});
    }
    if map.iter().any(Option::is_none){return Err(DomainError::QuiltingCoverageFailed)}
    let samples:Vec<_>=map.into_iter().map(|v|CorrespondenceSample{sources:[Some(WeightedSource{
        coordinate:v.expect("covered"),weight:1.0}),None,None,None]}).collect();
    let channels=compose_channels(r,&samples,s.output_width,s.output_height,cancel)?;
    let validity=source_validity(r,&samples,s.output_width,s.output_height,cancel)?;
    let min=f32::from(s.minimum_usable_confidence_milli)/1000.0;
    if validity.tiles().iter().flat_map(|t|&t.pixels).any(|v|v.0+1e-6<min){
        return Err(DomainError::UnusableQuiltingSource{rejected_candidates:rejected})}
    let boundary=correspondence_boundary_error(r,&pyramid,&samples,s.output_width,s.output_height);
    if boundary.0>s.max_boundary_periodicity_error_milli||boundary.1>s.max_boundary_periodicity_error_milli{
        return Err(DomainError::UnacceptableQuiltingBoundary{horizontal_milli:boundary.0,vertical_milli:boundary.1,
            maximum_milli:s.max_boundary_periodicity_error_milli})}
    let edge=r.source.base_color().tile_edge();
    let correspondence=CorrespondenceField::Registered(plane(s.output_width,s.output_height,edge,samples)?);
    let operation_values=owner.into_iter().map(|patch_index|DomainOperation::QuiltPatch{patch_index}).collect();
    let operations=OperationField::Registered(plane(s.output_width,s.output_height,edge,operation_values)?);
    let provenance=plane(s.output_width,s.output_height,edge,vec![ProvenanceValue::SeamComposed;count])?;
    let seam_mean=if seams.is_empty(){0}else{(seams.iter().map(|v|u64::from(v.normalized_energy_milli)).sum::<u64>()/seams.len() as u64)as u16};
    let source_usage:Vec<_>=uses.iter().map(|(&(source_x,source_y),&use_count)|SourcePatchUsage{source_x,source_y,use_count}).collect();
    let duplicate_patch_uses=source_usage.iter().map(|v|v.use_count.saturating_sub(1)).sum();
    let confidence=score(validity.tiles().iter().flat_map(|t|&t.pixels).map(|v|v.0).sum::<f32>()/count.max(1)as f32);
    let q=QuiltingDiagnostics{patch_size_pixels:(pw as u16,ph as u16),overlap_pixels:(ox as u16,oy as u16),
        placements,source_usage,overlap_seams:seams,rejected_unusable_candidates:rejected,duplicate_patch_uses,
        mean_seam_energy_milli:seam_mean,correspondence_confidence_milli:confidence,
        boundary_periodicity_error_milli:boundary,failure_reasons:Vec::new()};
    let diagnostics=DomainDiagnostics{selected_route:DomainRoute::TextureQuilting,cache_key:key.clone(),
        available_seam_terms:r.analysis.seamability.available_terms.clone(),normalized_weight_milli:normalized_weights(r),
        pass_through:None,seams:Vec::new(),boundary_cost_before_milli:(r.analysis.seamability.horizontal_cost_milli,
        r.analysis.seamability.vertical_cost_milli),boundary_cost_after_milli:boundary,
        messages:vec![format!("{} irregular seeded near-best patches; one shared seam/correspondence field drives all channels",q.placements.len())]};
    Ok(PreparedMaterialDomain{cache_key:key,prepared_source_digest:r.prepared_source_digest.clone(),analysis_digest:r.analysis.cache_key.clone(),
        route:DomainRoute::TextureQuilting,width:s.output_width,height:s.output_height,channels:DomainChannelStorage::Generated(channels),
        correspondence,operations,validity,provenance,seams:Vec::new(),quilting:Some(q),patch_match:None,diagnostics,
        qa_views:vec![DomainQaView::RegisteredChannels,DomainQaView::Quilting,DomainQaView::SourceUsage,DomainQaView::SeamCost,
            DomainQaView::SeamPath,DomainQaView::Correspondence,DomainQaView::Operations,DomainQaView::Validity,DomainQaView::BoundaryDifference],
        stage_result:StageResult::Executed{algorithm:AlgorithmProvenance{algorithm_id:STAGE_08B_QUILTING_ALGORITHM_ID.into(),
            version:STAGE_08B_ALGORITHM_VERSION.into()},settings_hash:ContentDigest::sha256(format!("{:?}|{}",s,r.seed).as_bytes()),diagnostics:Vec::new()}})
}

fn patch_size(r:&DomainRequest)->Result<(u32,u32),DomainError>{let b=r.source.base_color();let (x,y)=match r.quilting.patch_size{
    QuiltingPatchSize::RelativeMilli{width,height}=>(u64::from(b.width())*u64::from(width)/1000,u64::from(b.height())*u64::from(height)/1000),
    QuiltingPatchSize::PhysicalMicrometers{width,height}=>{let scale=r.scale_orientation.scale;
        if !scale.claims_world_accuracy()||matches!(scale.provenance,ScaleProvenance::PriorEstimated|ScaleProvenance::RelativeOnly){
            return incompatible("physical patch size requires world-accurate Stage 6 scale evidence, not a prior/relative estimate")}
        (u64::from(width)*scale.source_pixels_per_meter_x_milli.expect("world scale checked")/1_000_000_000,
         u64::from(height)*scale.source_pixels_per_meter_y_milli.expect("world scale checked")/1_000_000_000)},
};Ok((u32::try_from(x).map_err(|_|DomainError::ResourceLimitExceeded)?,u32::try_from(y).map_err(|_|DomainError::ResourceLimitExceeded)?))}

fn validate(r:&DomainRequest,pw:u32,ph:u32,ox:u32,oy:u32)->Result<(),DomainError>{let s=r.quilting;let b=r.source.base_color();
    let w=[s.weights.overlap,s.weights.histogram,s.weights.structure,s.weights.duplicate_use,s.weights.boundary_periodicity].into_iter().map(u64::from).sum::<u64>();
    if s.output_width==0||s.output_height==0||s.output_width>s.max_output_dimension||s.output_height>s.max_output_dimension||pw<4||ph<4||pw>b.width()||ph>b.height()||ox>=pw||oy>=ph||!(100..=750).contains(&s.overlap_milli)||s.pyramid_levels==0||s.pyramid_levels>8||s.candidate_count==0||s.candidate_count>s.max_candidate_count||s.near_best_count==0||s.near_best_count>s.candidate_count||s.near_best_threshold_milli>1000||s.minimum_usable_confidence_milli>1000||s.max_accepted_seam_cost_milli>1000||s.max_boundary_periodicity_error_milli>1000||w==0||s.max_patch_count==0||s.max_working_bytes==0||s.max_iterations==0||s.max_operations==0{return Err(DomainError::InvalidSettings)}
    match s.patch_size{QuiltingPatchSize::RelativeMilli{width,height}if width==0||width>1000||height==0||height>1000=>return Err(DomainError::InvalidSettings),QuiltingPatchSize::PhysicalMicrometers{width,height}if width==0||height==0=>return Err(DomainError::InvalidSettings),_=>{}}
    match s.semantics{
        QuiltingSemanticConstraint::UniqueDetail=>return incompatible("unique detail requires exact placement or constrained completion; quilting would smear it"),
        QuiltingSemanticConstraint::ManufacturedPattern=>return incompatible("manufactured motifs require a motif-period route"),
        QuiltingSemanticConstraint::MixedUnknown=>return incompatible("mixed/unknown semantics do not authorize quilting"),
        QuiltingSemanticConstraint::Directional{behavior,requested_angle_millidegrees:b,tolerance_millidegrees:t}=>{
            if !matches!(behavior,MaterialBehaviorClass::StochasticDirectional|MaterialBehaviorClass::OrganicDirectional){return incompatible("invalid directional behavior class")}
            let Some(a)=r.scale_orientation.global_orientation.axis_millidegrees else{return incompatible("directional quilting requires an authoritative Stage 6 orientation field")};
            if r.scale_orientation.global_orientation.confidence_milli==0{return incompatible("Stage 6 orientation confidence is insufficient")}
            if angular_delta(a as i32,b)>t{return incompatible("requested orientation exceeds Stage 6 tolerance")}},
        QuiltingSemanticConstraint::PeriodAlignedLattice{period_x:px,period_y:py,allow_period_aligned_quilting:allow}=>{
            if !allow{return incompatible("lattice routes to period-aligned crop or reconstruction")}
            if px==0||py==0||pw%u32::from(px)!=0||ph%u32::from(py)!=0||(pw-ox)%u32::from(px)!=0||(ph-oy)%u32::from(py)!=0{return incompatible("patch/overlap is not lattice-period aligned")}},
        QuiltingSemanticConstraint::Banded{axis,period_pixels:p}=>{if p==0{return Err(DomainError::InvalidSettings)}let a=if axis==BandAxis::Horizontal{ph-oy}else{pw-ox};if a%u32::from(p)!=0{return incompatible("patch advance crosses band phase")}},
        QuiltingSemanticConstraint::StochasticIsotropic=>{}}
    Ok(())}
fn incompatible<T>(s:&str)->Result<T,DomainError>{Err(DomainError::IncompatibleQuilting{reason:s.into()})}
fn angular_delta(a:i32,b:i32)->u32{let d=(i64::from(a)-i64::from(b)).unsigned_abs()%180_000;d.min(180_000-d)as u32}

fn build_registered_score_pyramid(r:&DomainRequest,cancel:&RenderCancellationToken)->Result<RegisteredScorePyramid,DomainError>{
    let base=r.source.base_color();let height=r.source.channels.iter().find_map(|c|match c{PreparedExemplarChannel::Scalar{role:MaterialChannelRole::Height,plane}=>Some(plane),_=>None});
    let rough=r.source.channels.iter().find_map(|c|match c{PreparedExemplarChannel::Scalar{role:MaterialChannelRole::Roughness,plane}=>Some(plane),_=>None});
    let normal=r.source.channels.iter().find_map(|c|match c{PreparedExemplarChannel::Normal{plane,..}=>Some(plane),_=>None});
    let mut pixels=Vec::with_capacity(pixel_count(base.width(),base.height())?);
    for y in 0..base.height(){if y%32==0{check_cancel(cancel)?}for x in 0..base.width(){let c=base.pixel(x,y).rgb;let n=normal.map_or([0.0,0.0,1.0],|p|p.pixel(x,y).xyz);
        pixels.push(ScorePixel{rgb:c.map(f64::from),gradient:0.0,height:height.map_or(0.0,|p|f64::from(p.pixel(x,y).0)),roughness:rough.map_or(0.0,|p|f64::from(p.pixel(x,y).0)),normal:n.map(f64::from),structure:structure_penalty(r,(x,y))})}}
    let mut levels=vec![ScoreLevel{width:base.width(),height:base.height(),pixels}];set_gradients(levels.last_mut().expect("level"));
    while levels.len()<usize::from(r.quilting.pyramid_levels)&&{let l=levels.last().unwrap();l.width>1||l.height>1}{check_cancel(cancel)?;let prior=levels.last().unwrap();let(w,h)=(prior.width.div_ceil(2),prior.height.div_ceil(2));let mut next=Vec::with_capacity((w*h)as usize);
        for y in 0..h{for x in 0..w{let mut p=ScorePixel::default();let mut n=0.0;for sy in y*2..(y*2+2).min(prior.height){for sx in x*2..(x*2+2).min(prior.width){let q=prior.pixels[(sy*prior.width+sx)as usize];for i in 0..3{p.rgb[i]+=q.rgb[i];p.normal[i]+=q.normal[i]}p.height+=q.height;p.roughness+=q.roughness;p.structure+=q.structure;n+=1.0}}for i in 0..3{p.rgb[i]/=n;p.normal[i]/=n}p.height/=n;p.roughness/=n;p.structure/=n;let len=(p.normal.iter().map(|v|v*v).sum::<f64>()).sqrt();p.normal=if len>1e-12{p.normal.map(|v|v/len)}else{[0.0,0.0,1.0]};next.push(p)}}let mut level=ScoreLevel{width:w,height:h,pixels:next};set_gradients(&mut level);levels.push(level)}
    Ok(RegisteredScorePyramid{levels})}
fn set_gradients(l:&mut ScoreLevel){let luminance=|p:ScorePixel|p.rgb[0]*0.2126+p.rgb[1]*0.7152+p.rgb[2]*0.0722;let copy=l.pixels.clone();for y in 0..l.height{for x in 0..l.width{let left=x.saturating_sub(1);let right=(x+1).min(l.width-1);let top=y.saturating_sub(1);let bottom=(y+1).min(l.height-1);let gx=luminance(copy[(y*l.width+right)as usize])-luminance(copy[(y*l.width+left)as usize]);let gy=luminance(copy[(bottom*l.width+x)as usize])-luminance(copy[(top*l.width+x)as usize]);l.pixels[(y*l.width+x)as usize].gradient=gx.hypot(gy)*0.5}}}
fn pyramid_pixel(p:&RegisteredScorePyramid,level:usize,c:SourceCoordinate)->ScorePixel{let l=&p.levels[level.min(p.levels.len()-1)];let scale=1u32<<level.min(31);let x=(c.x/scale).min(l.width-1);let y=(c.y/scale).min(l.height-1);l.pixels[(y*l.width+x)as usize]}
fn pyramid_cost(r:&DomainRequest,p:&RegisteredScorePyramid,level:usize,a:SourceCoordinate,b:SourceCoordinate)->f64{let a=pyramid_pixel(p,level,a);let b=pyramid_pixel(p,level,b);let w=normalized_weight_f64(r);let color=(0..3).map(|i|(a.rgb[i]-b.rgb[i]).abs()).sum::<f64>()/3.0;let normal=(1.0-a.normal.iter().zip(b.normal).map(|(x,y)|x*y).sum::<f64>()).clamp(0.0,2.0)*0.5;w[0]*color+w[1]*(a.gradient-b.gradient).abs()+w[2]*(a.height-b.height).abs()+w[3]*normal+w[4]*(a.roughness-b.roughness).abs()+w[5]*a.structure.max(b.structure)}
fn distribution_error(p:&RegisteredScorePyramid,src:SourceCoordinate,w:u32,h:u32)->(f64,f64){let mut total_hist=0.0_f64;let mut total_structure=0.0_f64;for(level_index,l)in p.levels.iter().enumerate(){let scale=1u32<<level_index.min(31);let sx=(src.x/scale).min(l.width-1);let sy=(src.y/scale).min(l.height-1);let rw=w.div_ceil(scale).min(l.width-sx).max(1);let rh=h.div_ceil(scale).min(l.height-sy).max(1);let mut patch=[0.0_f64;24];let mut all=[0.0_f64;24];let(mut ps,mut pc,mut ac)=(0.0_f64,0.0_f64,0.0_f64);for y in 0..l.height{for x in 0..l.width{let q=l.pixels[(y*l.width+x)as usize];let lum=(q.rgb[0]*0.2126+q.rgb[1]*0.7152+q.rgb[2]*0.0722).clamp(0.0,0.999_999);all[(lum*16.0)as usize]+=1.0;all[16+(q.gradient.clamp(0.0,0.999_999)*8.0)as usize]+=1.0;ac+=1.0;if x>=sx&&x<sx+rw&&y>=sy&&y<sy+rh{patch[(lum*16.0)as usize]+=1.0;patch[16+(q.gradient.clamp(0.0,0.999_999)*8.0)as usize]+=1.0;ps+=q.structure;pc+=1.0}}}for i in 0..24{patch[i]/=pc.max(1.0);all[i]/=ac.max(1.0);total_hist+=(patch[i]-all[i]).abs()}let all_structure=l.pixels.iter().map(|q|q.structure).sum::<f64>()/ac.max(1.0);total_structure+=(ps/pc.max(1.0)-all_structure).abs()}let n=p.levels.len().max(1)as f64;(total_hist/(4.0*n),total_structure/n)}

fn preflight(r:&DomainRequest,pw:u32,ph:u32)->Result<(),DomainError>{let s=r.quilting;let p=u64::from(s.output_width).checked_mul(u64::from(s.output_height)).ok_or(DomainError::ResourceLimitExceeded)?;if p>s.max_output_pixels{return Err(DomainError::ResourceLimitExceeded)}let bytes=p.checked_mul(96+r.source.channels.len()as u64*24).ok_or(DomainError::ResourceLimitExceeded)?;let ops=p.checked_mul(12+r.source.channels.len()as u64*5).and_then(|v|v.checked_add(u64::from(pw)*u64::from(ph)*u64::from(s.candidate_count)*u64::from(s.pyramid_levels)*8)).ok_or(DomainError::ResourceLimitExceeded)?;if bytes>s.max_working_bytes||ops>s.max_operations{Err(DomainError::ResourceLimitExceeded)}else{Ok(())}}

fn rects(r:&DomainRequest,pw:u32,ph:u32,ox:u32,oy:u32)->Result<Vec<Rect>,DomainError>{let s=r.quilting;let fixed=matches!(s.semantics,QuiltingSemanticConstraint::PeriodAlignedLattice{..}|QuiltingSemanticConstraint::Banded{..});let mut out=Vec::new();let mut y=0;let mut row=0u64;while y<s.output_height{let mut x=0;let mut col=0u64;while x<s.output_width{if out.len()as u32>=s.max_patch_count{return Err(DomainError::QuiltingCoverageFailed)}out.push(Rect{x,y,w:pw.min(s.output_width-x),h:ph.min(s.output_height-y)});if x+pw>=s.output_width{break}let j=if fixed{0}else{jitter(splitmix(r.seed^row.rotate_left(17)^col),ox/3)};x=advance(x,pw-ox,j,pw,s.output_width);col+=1}if y+ph>=s.output_height{break}let j=if fixed{0}else{jitter(splitmix(r.seed^row^0x9e37_79b9),oy/3)};y=advance(y,ph-oy,j,ph,s.output_height);row+=1}Ok(out)}
fn advance(v:u32,base:u32,j:i32,patch:u32,out:u32)->u32{v.saturating_add((i64::from(base)+i64::from(j)).max(1)as u32).min(out-1).min(v+patch-2)}
fn jitter(v:u64,e:u32)->i32{if e==0{0}else{(v%u64::from(e*2+1))as i32-e as i32}}

#[allow(clippy::too_many_arguments)]
fn candidates(r:&DomainRequest,pyramid:&RegisteredScorePyramid,rect:Rect,pw:u32,ph:u32,map:&[Option<SourceCoordinate>],uses:&BTreeMap<(u32,u32),u32>,rejected:&mut u32,ops:&mut u64,cancel:&RenderCancellationToken)->Result<Vec<Candidate>,DomainError>{
    let s=r.quilting;let b=r.source.base_color();let sx=b.width()-pw+1;let sy=b.height()-ph+1;
    let stream=splitmix(r.seed^u64::from(rect.x).rotate_left(13)^u64::from(rect.y).rotate_left(31));let mut out=Vec::new();
    for attempt in 0..u32::from(s.candidate_count).saturating_mul(8){if out.len()>=s.candidate_count as usize{break}if attempt%32==0{check_cancel(cancel)?}let m=splitmix(stream^u64::from(attempt));let x=m as u32%sx;let y=(m>>32)as u32%sy;if out.iter().any(|c:&Candidate|c.x==x&&c.y==y){continue}if !usable(r,x,y,rect.w,rect.h){*rejected+=1;continue}let cost=candidate_cost(r,pyramid,rect,SourceCoordinate{x,y},map,uses,ops)?;out.push(Candidate{x,y,cost})}
    if out.is_empty(){return Err(DomainError::UnusableQuiltingSource{rejected_candidates:*rejected})}out.sort_by(|a,b|a.cost.total_cmp(&b.cost).then(a.y.cmp(&b.y)).then(a.x.cmp(&b.x)));Ok(out)}

fn usable(r:&DomainRequest,sx:u32,sy:u32,w:u32,h:u32)->bool{let p=r.analysis.usability.confidence.level(0).expect("validated");let min=f32::from(r.quilting.minimum_usable_confidence_milli)/1000.0;for y in 0..h{for x in 0..w{let mut v=p.pixel(sx+x,sy+y).0;if let Some(c)=&r.source.coverage{v*=c.pixel(sx+x,sy+y).0}if v+1e-6<min{return false}}}true}

fn candidate_cost(r:&DomainRequest,pyramid:&RegisteredScorePyramid,rect:Rect,src:SourceCoordinate,map:&[Option<SourceCoordinate>],uses:&BTreeMap<(u32,u32),u32>,ops:&mut u64)->Result<f64,DomainError>{let s=r.quilting;let w=s.weights;let sum=f64::from(w.overlap+w.histogram+w.structure+w.duplicate_use+w.boundary_periodicity).max(1.0);let mut overlap=0.0;let mut n=0u64;for level in 0..pyramid.levels.len(){let scale=1u32<<level.min(31);for ly in 0..rect.h.div_ceil(scale){for lx in 0..rect.w.div_ceil(scale){let(lx0,ly0)=((lx*scale).min(rect.w-1),(ly*scale).min(rect.h-1));if let Some(old)=map[((rect.y+ly0)*s.output_width+rect.x+lx0)as usize]{overlap+=pyramid_cost(r,pyramid,level,old,SourceCoordinate{x:src.x+lx0,y:src.y+ly0});n+=1}}}}*ops=ops.saturating_add(n);if *ops>s.max_operations{return Err(DomainError::ResourceLimitExceeded)}overlap/=n.max(1)as f64;let(histogram,structure)=distribution_error(pyramid,src,rect.w,rect.h);let duplicate=f64::from(*uses.get(&(src.x,src.y)).unwrap_or(&0))/(1.0+uses.values().sum::<u32>()as f64);let boundary=boundary_candidate(r,pyramid,rect,src,map);Ok((f64::from(w.overlap)*overlap+f64::from(w.histogram)*histogram+f64::from(w.structure)*structure+f64::from(w.duplicate_use)*duplicate+f64::from(w.boundary_periodicity)*boundary)/sum)}

fn boundary_candidate(r:&DomainRequest,pyramid:&RegisteredScorePyramid,rect:Rect,src:SourceCoordinate,map:&[Option<SourceCoordinate>])->f64{let s=r.quilting;let(mut total,mut n)=(0.0,0u64);if rect.x+rect.w==s.output_width{for y in 0..rect.h{if let Some(left)=map[((rect.y+y)*s.output_width)as usize]{total+=pyramid_cost(r,pyramid,0,left,SourceCoordinate{x:src.x+rect.w-1,y:src.y+y});n+=1}}}if rect.y+rect.h==s.output_height{for x in 0..rect.w{if let Some(top)=map[(rect.x+x)as usize]{total+=pyramid_cost(r,pyramid,0,top,SourceCoordinate{x:src.x+x,y:src.y+rect.h-1});n+=1}}}total/n.max(1)as f64}
fn near_best(cs:&[Candidate],seed:u64,placement:u64,s:QuiltingSettings)->usize{let threshold=cs[0].cost+f64::from(s.near_best_threshold_milli)/1000.0;let n=cs.iter().take(usize::from(s.near_best_count)).take_while(|c|c.cost<=threshold).count().max(1);(splitmix(seed^placement.rotate_left(23)^0xd1b5_4a32_d192_ed03)%n as u64)as usize}

fn solve_seam(r:&DomainRequest,pyramid:&RegisteredScorePyramid,rect:Rect,src:SourceCoordinate,axis:SeamAxis,overlap:u32,map:&[Option<SourceCoordinate>],pi:u32,cancel:&RenderCancellationToken)->Result<QuiltOverlapSeam,DomainError>{let length=if axis==SeamAxis::X{rect.h}else{rect.w};let candidates=overlap.max(1);let mut previous=vec![0.0;candidates as usize];let mut back=vec![vec![0u16;candidates as usize];length as usize];for p in 0..candidates{previous[p as usize]=overlap_cost(r,pyramid,rect,src,axis,0,p,map)}for line in 1..length{if line%32==0{check_cancel(cancel)?}let mut current=vec![f64::INFINITY;candidates as usize];for p in 0..candidates{let(mut best,mut bp)=(f64::INFINITY,0);for prior in p.saturating_sub(1)..=(p+1).min(candidates-1){let v=previous[prior as usize]+f64::from(p.abs_diff(prior))*0.025;if v<best||(v==best&&prior<bp){best=v;bp=prior}}current[p as usize]=best+overlap_cost(r,pyramid,rect,src,axis,line,p,map);back[line as usize][p as usize]=bp as u16}previous=current}let end=(0..candidates).min_by(|a,b|previous[*a as usize].total_cmp(&previous[*b as usize]).then(a.cmp(b))).unwrap_or(0);let mut positions=vec![0u16;length as usize];positions[length as usize-1]=end as u16;for line in(1..length).rev(){positions[line as usize-1]=back[line as usize][positions[line as usize]as usize]}Ok(QuiltOverlapSeam{patch_index:pi,axis,output_origin:(rect.x,rect.y),positions,normalized_energy_milli:score((previous[end as usize]/f64::from(length.max(1)))as f32)})}
fn overlap_cost(r:&DomainRequest,pyramid:&RegisteredScorePyramid,rect:Rect,src:SourceCoordinate,axis:SeamAxis,line:u32,p:u32,map:&[Option<SourceCoordinate>])->f64{let(lx,ly)=if axis==SeamAxis::X{(p,line)}else{(line,p)};map[((rect.y+ly)*r.quilting.output_width+rect.x+lx)as usize].map_or(1.0,|old|pyramid_cost(r,pyramid,0,old,SourceCoordinate{x:src.x+lx,y:src.y+ly}))}
#[allow(clippy::too_many_arguments)]
fn apply(rect:Rect,src:SourceCoordinate,pi:u32,vs:Option<&QuiltOverlapSeam>,hs:Option<&QuiltOverlapSeam>,ow:u32,map:&mut[Option<SourceCoordinate>],owner:&mut[u32]){for ly in 0..rect.h{for lx in 0..rect.w{let i=((rect.y+ly)*ow+rect.x+lx)as usize;let v=vs.is_none_or(|s|lx>=u32::from(s.positions[ly as usize]));let h=hs.is_none_or(|s|ly>=u32::from(s.positions[lx as usize]));if map[i].is_none()||(v&&h){map[i]=Some(SourceCoordinate{x:src.x+lx,y:src.y+ly});owner[i]=pi}}}}

fn correspondence_boundary_error(r:&DomainRequest,p:&RegisteredScorePyramid,samples:&[CorrespondenceSample],w:u32,h:u32)->(u16,u16){let coord=|x:u32,y:u32|samples[(y*w+x)as usize].sources[0].expect("quilt correspondence").coordinate;let x=(0..h).map(|y|pyramid_cost(r,p,0,coord(0,y),coord(w-1,y))).sum::<f64>()/f64::from(h.max(1));let y=(0..w).map(|x|pyramid_cost(r,p,0,coord(x,0),coord(x,h-1))).sum::<f64>()/f64::from(w.max(1));(score(x as f32),score(y as f32))}
fn splitmix(mut v:u64)->u64{v=v.wrapping_add(0x9e37_79b9_7f4a_7c15);v=(v^(v>>30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);v=(v^(v>>27)).wrapping_mul(0x94d0_49bb_1331_11eb);v^(v>>31)}
