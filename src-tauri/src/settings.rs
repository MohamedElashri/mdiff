use crate::engine::Engine;
use crate::workspaces::Credentials;
use serde_json::Value;
use std::sync::Arc;
use tauri::{Emitter, Manager, State, WebviewWindow};
#[derive(Default)]
pub struct MemorySettings(tokio::sync::Mutex<Memory>);
#[derive(Default)]
struct Memory {
    revision: u64,
    credentials: Credentials,
}
impl MemorySettings {
    pub(crate) async fn credentials(&self) -> Credentials {
        self.0.lock().await.credentials.clone()
    }
}
fn validate_credentials(value: &Credentials) -> Result<(), String> {
    for host in [&value.token_host, &value.gitlab_host]
        .into_iter()
        .flatten()
    {
        let url =
            url::Url::parse(&format!("https://{host}/")).map_err(|_| "Invalid connection host")?;
        if host.len() > 255
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || host.contains(['/', '\\', '@'])
        {
            return Err(
                "Use a hostname with an optional port, without a URL or credentials".into(),
            );
        }
    }
    for token in [&value.token, &value.gitlab_token].into_iter().flatten() {
        if token.len() > 16 * 1024 || token.contains(['\n', '\r', '\0']) {
            return Err("Invalid connection token".into());
        }
    }
    Ok(())
}
async fn store(
    app: &tauri::AppHandle,
    state: &Engine,
) -> Result<Arc<diffle::user_config::UserConfigStore>, String> {
    let path = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("review.json");
    Ok(state.config(path).await)
}
#[tauri::command]
pub async fn desktop_settings(
    app: tauri::AppHandle,
    state: State<'_, Engine>,
    window: WebviewWindow,
    memory: State<'_, MemorySettings>,
) -> Result<Value, String> {
    crate::windows::authorize(&window)?;
    let _transition = state.lifecycle.lock().await;
    let config = store(&app, &state).await?;
    let mut value = serde_json::to_value(config.reload().await.map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    value["githubEnabled"] = crate::github::Connections::load(config.file())
        .await
        .map_err(|e| e.to_string())?
        .github_enabled
        .into();
    let memory = memory.0.lock().await;
    value["settingsRevision"] = memory.revision.into();
    value["connectionProfiles"] =
        serde_json::to_value(&memory.credentials).map_err(|e| e.to_string())?;
    Ok(value)
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewPatch {
    auto_viewed: Option<Vec<String>>,
    context_lines: Option<i64>,
    follow_refs: Option<diffle::protocol::FollowRefs>,
    line_wrap: Option<bool>,
}
#[tauri::command]
pub async fn review_preferences(
    app: tauri::AppHandle,
    state: State<'_, Engine>,
    window: WebviewWindow,
    sessions: State<'_, crate::workspaces::Sessions>,
    settings: Option<ReviewPatch>,
) -> Result<diffle::protocol::UserConfig, String> {
    crate::windows::authorize(&window)?;
    let _work = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    let config = store(&app, &state).await?;
    if let Some(settings) = settings {
        app.state::<crate::windows::Windows>().writable()?;
        if settings
            .context_lines
            .is_some_and(|n| !(0..=10000).contains(&n))
        {
            return Err("Context lines must be between 0 and 10000".into());
        }
        config
            .set(diffle::user_config::UserConfigPatch {
                auto_viewed: settings.auto_viewed,
                context_lines: settings.context_lines,
                follow_refs: settings.follow_refs,
                line_wrap: settings.line_wrap,
            })
            .await
            .map_err(|e| e.to_string())
    } else {
        config.reload().await.map_err(|e| e.to_string())
    }
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Native states/window are injected; keep the settings wire fields explicit.
pub async fn save_desktop_settings(
    app: tauri::AppHandle,
    state: State<'_, Engine>,
    lsp_commands: Option<Value>,
    github_hosts: Option<Value>,
    github_enabled: Option<bool>,
    window: WebviewWindow,
    memory: State<'_, MemorySettings>,
    sessions: State<'_, crate::workspaces::Sessions>,
    revision: u64,
    connection_profiles: Option<Credentials>,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    app.state::<crate::windows::Windows>().writable()?;
    if let Some(profile) = &connection_profiles {
        validate_credentials(profile)?;
    }
    let hosting = app.state::<crate::hosting::Hosting>();
    let connection_edit =
        github_hosts.is_some() || github_enabled.is_some() || connection_profiles.is_some();
    let _pause = connection_edit.then(|| hosting.broker.pause());
    if connection_edit {
        hosting.broker.clear().await;
    }
    let _work = sessions.gate.write().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    let commands = lsp_commands
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| format!("Invalid language server settings: {e}"))?;
    let hosts = github_hosts
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| format!("Invalid GitHub hosts: {e}"))?;
    let _transition = state.lifecycle.lock().await;
    let mut memory = memory.0.lock().await;
    if memory.revision != revision {
        return Err(
            "Connection/tool settings changed in another window. Reopen Preferences before saving"
                .into(),
        );
    }
    let config = store(&app, &state).await?;
    let previous_hosts = config.get().github_hosts;
    let previous_enabled = crate::github::Connections::load(config.file())
        .await
        .map_err(|e| e.to_string())?
        .github_enabled;
    let github_enabled = github_enabled.unwrap_or(previous_enabled);
    let mut connection_profiles = connection_profiles.unwrap_or_else(|| memory.credentials.clone());
    config
        .set_desktop_fields(commands, hosts)
        .await
        .map_err(|e| e.to_string())?;
    crate::github::Connections { github_enabled }
        .save(config.file())
        .await
        .map_err(|e| e.to_string())?;
    if !github_enabled {
        connection_profiles.token = None;
    }
    let old = std::mem::replace(&mut memory.credentials, connection_profiles.clone());
    memory.revision += 1;
    let next_revision = memory.revision;
    drop(memory);
    let github_changed =
        old.token != connection_profiles.token || old.token_host != connection_profiles.token_host;
    let gitlab_changed = old.gitlab_token != connection_profiles.gitlab_token
        || old.gitlab_host != connection_profiles.gitlab_host;
    let auth_changed = github_changed
        || gitlab_changed
        || previous_enabled != github_enabled
        || previous_hosts != config.get().github_hosts;
    state.settings_changed(github_enabled, auth_changed).await;
    for (gitlab, changed) in [(false, github_changed), (true, gitlab_changed)] {
        if changed {
            for profile in [
                Credentials {
                    token: None,
                    gitlab_token: None,
                    ..old.clone()
                },
                connection_profiles.clone(),
            ] {
                state
                    .check_credentials(&crate::engine::OpenOptions {
                        path: ".".into(),
                        request: diffle::protocol::EnterRequest::Working,
                        gitlab_url: gitlab.then(|| "credentials".into()),
                        tab_scope: None,
                        token: profile.token.clone(),
                        token_host: profile.token_host.clone(),
                        gitlab_token: profile.gitlab_token.clone(),
                        gitlab_host: profile.gitlab_host.clone(),
                    })
                    .await?;
            }
        }
    }
    let _ = app.emit(
        "desktop-settings",
        serde_json::json!({"revision":next_revision,"authChanged":auth_changed}),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_preferences_cannot_set_credentials_or_language_server_trust() {
        for value in [
            serde_json::json!({"token":"secret"}),
            serde_json::json!({"lspCommands":{}}),
            serde_json::json!({"trust":true}),
        ] {
            assert!(serde_json::from_value::<ReviewPatch>(value).is_err());
        }
        assert!(serde_json::from_value::<ReviewPatch>(
            serde_json::json!({"contextLines":9,"lineWrap":false})
        )
        .is_ok());
    }
    #[tokio::test]
    async fn connection_profiles_are_process_owned_and_host_validation_excludes_credentials_and_paths(
    ) {
        let memory = MemorySettings::default();
        let credentials = Credentials {
            token: Some("private-test-token".into()),
            token_host: Some("github.example".into()),
            gitlab_token: Some("private-gl-token".into()),
            gitlab_host: Some("gitlab.example:8443".into()),
        };
        validate_credentials(&credentials).unwrap();
        memory.0.lock().await.credentials = credentials.clone();
        assert_eq!(
            memory.credentials().await.token_host.as_deref(),
            Some("github.example")
        );
        assert!(MemorySettings::default()
            .credentials()
            .await
            .token
            .is_none());
        for host in [
            "https://github.example",
            "token@github.example",
            "github.example/path",
            "github.example?token=secret",
        ] {
            assert!(validate_credentials(&Credentials {
                token_host: Some(host.into()),
                ..Default::default()
            })
            .is_err());
        }
        assert!(validate_credentials(&Credentials {
            token: Some("value\nheader".into()),
            ..Default::default()
        })
        .is_err());
    }
}
