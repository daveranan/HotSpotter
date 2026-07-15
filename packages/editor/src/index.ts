export const WORKFLOW_STEPS = [
  "open_image",
  "mark_patches",
  "layout",
  "generate_maps",
  "polish",
  "preview",
  "export",
] as const;

export type WorkflowStep = (typeof WORKFLOW_STEPS)[number];

