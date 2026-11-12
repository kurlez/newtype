fn main() {
    let commit = std::env::var("NEWTYPE_COMMIT")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(git_head);
    let commit = sanitize_commit(&commit);
    println!("cargo:rustc-env=NEWTYPE_COMMIT={commit}");
    println!("cargo:rerun-if-env-changed=NEWTYPE_COMMIT");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_state",
            "set_channel",
            "check_for_updates",
            "apply_update",
            "pipeline_status",
            "dictation_snapshot",
            "dictation_save",
            "dictation_set_key",
            "dictation_clear_key",
            "dictation_test_connection",
            "dictation_list_models",
            "dictation_request_access",
            "dictation_open_privacy",
            "dictation_capture_hotkey",
            "dictation_cancel_capture",
            "dictation_capture_key",
            "dictation_set_hotkey",
            "dictation_record_toggle",
            "dictation_cancel",
            "dictation_history",
            "dictation_delete_history",
            "dictation_copy_history",
            "dictation_reinsert_history",
            "dictation_clear_history",
            "dictation_rerun_history",
            "dictation_export_dictionary",
            "dictation_import_dictionary",
            "dictation_microphones",
            "dictation_probe_microphone",
            "dictation_reset_models",
            "local_download",
            "local_cancel_download",
            "local_delete",
            "local_skip_download",
            "local_set_mirror",
            "local_use_model",
            "indicator_save_position",
            "indicator_open_history",
        ]),
    ))
    .expect("failed to run tauri-build");
}

fn git_head() -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8(output.stdout).unwrap_or_default()
        }
        _ => "unknown".into(),
    }
}

fn sanitize_commit(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        trimmed.chars().take(7).collect()
    } else {
        "unknown".into()
    }
}
