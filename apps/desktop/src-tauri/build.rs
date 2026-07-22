// SPDX-License-Identifier: GPL-3.0-only

fn main() {
    const COMMANDS: &[&str] = &[
        "about",
        "app_bootstrap",
        "profile_list",
        "profile_create",
        "source_select",
        "import_prepare",
        "import_jobs",
        "import_cancel",
    ];

    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes).expect("failed to build the OPAP Tauri context");
}
