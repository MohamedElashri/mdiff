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
    let mut value = serde_json::to_value(config.get()).map_err(|e| e.to_string())?;
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
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Native states/window are injected; keep the settings wire fields explicit.
pub async fn save_desktop_settings(
    app: tauri::AppHandle,
    state: State<'_, Engine>,
    lsp_commands: Value,
    github_hosts: Value,
    github_enabled: bool,
    window: WebviewWindow,
    memory: State<'_, MemorySettings>,
    sessions: State<'_, crate::workspaces::Sessions>,
    revision: u64,
    mut connection_profiles: Credentials,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    app.state::<crate::windows::Windows>().writable()?;
    validate_credentials(&connection_profiles)?;
    let hosting = app.state::<crate::hosting::Hosting>();
    let _pause = hosting.broker.pause();
    hosting.broker.clear().await;
    let _work = sessions.gate.write().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    let commands = serde_json::from_value(lsp_commands)
        .map_err(|e| format!("Invalid language server settings: {e}"))?;
    let hosts =
        serde_json::from_value(github_hosts).map_err(|e| format!("Invalid GitHub hosts: {e}"))?;
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
    config
        .set_desktop_commands(commands, hosts)
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
    state.settings_changed(github_enabled).await;
    let github_changed =
        old.token != connection_profiles.token || old.token_host != connection_profiles.token_host;
    let gitlab_changed = old.gitlab_token != connection_profiles.gitlab_token
        || old.gitlab_host != connection_profiles.gitlab_host;
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
    let _=app.emit("desktop-settings",serde_json::json!({"revision":next_revision,"authChanged":github_changed||gitlab_changed||previous_enabled!=github_enabled||previous_hosts!=config.get().github_hosts}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
