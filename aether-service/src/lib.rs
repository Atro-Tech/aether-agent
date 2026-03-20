// Æther Agent service.
//
// Two jobs:
//   1. Parse add-in manifests (TOML → struct)
//   2. Write context.json so the agent inside knows what tools it has

pub mod context;
pub mod manifest;
