use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    BaseColor,
    Normal,
    Height,
    Roughness,
    Metallic,
    AmbientOcclusion,
    RegionId,
    MaterialId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelDataKind {
    Color,
    LinearData,
    FlatId,
    Vector,
}

impl Channel {
    #[must_use]
    pub const fn data_kind(self) -> ChannelDataKind {
        match self {
            Self::BaseColor => ChannelDataKind::Color,
            Self::Normal => ChannelDataKind::Vector,
            Self::RegionId | Self::MaterialId => ChannelDataKind::FlatId,
            Self::Height | Self::Roughness | Self::Metallic | Self::AmbientOcclusion => {
                ChannelDataKind::LinearData
            }
        }
    }

    #[must_use]
    pub const fn is_estimated(self) -> bool {
        !matches!(self, Self::BaseColor | Self::RegionId | Self::MaterialId)
    }
}
