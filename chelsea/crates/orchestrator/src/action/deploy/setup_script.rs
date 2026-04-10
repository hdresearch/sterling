//! Build the shell script that runs on the VM to set up a deployed project.
//!
//! Ported from `vers-landing/src/lib/server/services/github.service.ts` —
//! `buildSetupCommand()` and related helpers.

use super::DeploySettings;
use std::collections::HashMap;

/// Detect which tool families are referenced in the commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ToolFamily {
    Node,
    Python,
    Rust,
    Go,
}

/// Extract the first space-separated token (tool name) from a command.
fn extract_tool(cmd: &str) -> &str {
    let token = cmd.trim().split_whitespace().next().unwrap_or("");
    // Handle absolute paths: /usr/bin/npm → npm
    token.rsplit('/').next().unwrap_or(token)
}

fn tool_family(tool: &str) -> Option<ToolFamily> {
    match tool {
        "npm" | "npx" | "yarn" | "pnpm" | "bun" | "bunx" | "node" => Some(ToolFamily::Node),
        "python" | "python3" | "pip" | "pip3" | "uv" => Some(ToolFamily::Python),
        "cargo" => Some(ToolFamily::Rust),
        "go" => Some(ToolFamily::Go),
        _ => None,
    }
}

fn needs_explicit_install(tool: &str) -> bool {
    matches!(tool, "yarn" | "pnpm" | "bun" | "bunx" | "uv")
}

fn runtime_install_commands(family: ToolFamily) -> Vec<&'static str> {
    match family {
        ToolFamily::Node => vec![
            "if ! command -v node >/dev/null 2>&1; then curl -fsSL https://deb.nodesource.com/setup_lts.x | bash - && apt-get install -y -qq nodejs; fi",
        ],
        ToolFamily::Python => vec![
            "if ! command -v python3 >/dev/null 2>&1; then apt-get install -y -qq python3 python3-pip python3-venv; fi",
        ],
        ToolFamily::Rust => vec![
            "if ! command -v cargo >/dev/null 2>&1; then curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; fi",
            "test -f $HOME/.cargo/env && . $HOME/.cargo/env || true",
        ],
        ToolFamily::Go => vec![
            "if ! command -v go >/dev/null 2>&1; then curl -fsSL https://go.dev/dl/go1.22.5.linux-amd64.tar.gz | tar -C /usr/local -xzf -; fi",
            "export PATH=$PATH:/usr/local/go/bin",
        ],
    }
}

fn pkg_manager_install_commands(tool: &str) -> Vec<&'static str> {
    match tool {
        "yarn" => vec!["command -v yarn >/dev/null 2>&1 || npm install -g yarn"],
        "pnpm" => vec!["command -v pnpm >/dev/null 2>&1 || npm install -g pnpm"],
        "bun" | "bunx" => vec![
            "if ! command -v bun >/dev/null 2>&1; then curl -fsSL https://bun.sh/install | bash; fi",
            "export PATH=$HOME/.bun/bin:$PATH",
        ],
        "uv" => vec![
            "if ! command -v uv >/dev/null 2>&1; then curl -LsSf https://astral.sh/uv/install.sh | sh; fi",
            "test -f $HOME/.local/bin/env && . $HOME/.local/bin/env || export PATH=$HOME/.local/bin:$PATH",
        ],
        _ => vec![],
    }
}

/// Build the nginx reverse-proxy config section of the setup script.
fn nginx_reverse_proxy_section() -> String {
    // Poll for the app's listening port via /etc/VERS_PID
    let poll = r#"VERS_TRIES=0
while [ $VERS_TRIES -lt 60 ]; do
  VERS_TRIES=$((VERS_TRIES+1))
  VERS_APP_PID=$(cat /etc/VERS_PID 2>/dev/null)
  if [ -n "$VERS_APP_PID" ]; then
    VERS_ALL_PIDS=$(pgrep -g $(ps -o pgid= -p $VERS_APP_PID 2>/dev/null | tr -d ' ') 2>/dev/null || echo $VERS_APP_PID $(pgrep -P $VERS_APP_PID 2>/dev/null))
    VERS_PID_PATTERN=$(echo $VERS_ALL_PIDS | tr -s ' ' '\n' | sort -u | tr '\n' '|' | sed 's/|$//')
    if [ -n "$VERS_PID_PATTERN" ]; then
      VERS_PORT=$(ss -tlnp 2>/dev/null | grep -E "pid=($VERS_PID_PATTERN)" | awk '{print $4}' | rev | cut -d: -f1 | rev | head -1)
      if [ -n "$VERS_PORT" ] && [ "$VERS_PORT" -gt 0 ] 2>/dev/null; then
        echo $VERS_PORT > /etc/VERS_PORT
        break
      fi
    fi
  fi
  sleep 1
done"#;

    let nginx_config = r#"test -f /etc/VERS_PORT && VERS_PORT=$(cat /etc/VERS_PORT)
test -n "$VERS_PORT"

cat > /etc/nginx/sites-available/default << 'NGINX_EOF'
server {
    listen 80 default_server;
    listen [::]:80 default_server;

    server_name _;

    location / {
        proxy_pass http://127.0.0.1:VERS_PORT_PLACEHOLDER;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}

map $http_upgrade $connection_upgrade {
    default upgrade;
    "" close;
}
NGINX_EOF

sed -i "s/VERS_PORT_PLACEHOLDER/$VERS_PORT/g" /etc/nginx/sites-available/default
rm -f /etc/nginx/sites-enabled/default
ln -sf /etc/nginx/sites-available/default /etc/nginx/sites-enabled/default
nginx -t
(systemctl restart nginx 2>/dev/null || service nginx restart 2>/dev/null || nginx)"#;

    format!("{poll}\n{nginx_config}")
}

/// Build the nginx static-file section (when no run command is given).
fn nginx_static_section(serve_path: &str) -> String {
    format!(
        r#"cat > /etc/nginx/sites-available/default << 'NGINX_EOF'
server {{
    listen 80 default_server;
    listen [::]:80 default_server;

    root {serve_path};
    index index.html index.htm;

    server_name _;

    location / {{
        try_files $uri $uri/ /index.html =404;
    }}
}}
NGINX_EOF
rm -f /etc/nginx/sites-enabled/default
ln -sf /etc/nginx/sites-available/default /etc/nginx/sites-enabled/default
nginx -t
(systemctl restart nginx 2>/dev/null || service nginx restart 2>/dev/null || nginx)"#
    )
}

/// Escape a value for safe inclusion in a shell single-quoted string.
/// The only character that needs escaping inside single quotes is the
/// single quote itself: `'` → `'\''` (end quote, escaped quote, start quote).
fn shell_escape(value: &str) -> String {
    value.replace('\'', "'\\''")
}

/// Build the block that writes user environment variables to `/etc/environment`
/// and exports them for the current script.
fn build_env_vars_block(env_vars: &HashMap<String, String>) -> Vec<String> {
    if env_vars.is_empty() {
        return vec![];
    }

    let mut lines = Vec::new();
    lines.push("# Write user environment variables to /etc/environment".to_string());

    // Atomically overwrite /etc/environment via a temp file
    lines.push("cat > /tmp/vers_env_vars << 'VERS_ENV_EOF'".to_string());
    for (key, value) in env_vars.into_iter() {
        // Inside a heredoc the values are literal (no expansion with 'VERS_ENV_EOF'),
        // so we only need to ensure the value doesn't contain the delimiter.
        // /etc/environment format: KEY=value (unquoted, one per line).
        // Values with spaces/special chars work because /etc/environment is
        // parsed by PAM, which handles them correctly.
        lines.push(format!("{}={}", key, value));
    }
    lines.push("VERS_ENV_EOF".to_string());
    lines.push("cp /tmp/vers_env_vars /etc/environment && rm /tmp/vers_env_vars".to_string());
    lines.push(String::new());

    // Also export for the current script so install/build/run commands see them
    for (key, value) in env_vars.into_iter() {
        lines.push(format!("export {}='{}'", key, shell_escape(&value)));
    }
    lines.push(String::new());

    lines
}

/// Build the complete setup shell script for a deploy.
///
/// This script is written to `/tmp/vers_deploy.sh` and run in the background.
/// It writes `/etc/VERS_DEPLOY_STATUS` with `done` or `failed: <reason>` on completion.
pub fn build_setup_script(
    clone_url: &str,
    branch: &str,
    settings: &DeploySettings,
    env_vars: &HashMap<String, String>,
) -> String {
    let project_path = "/home/user/project";

    // Determine the serve path (working_directory relative to project root)
    let serve_path = match settings.working_directory.as_deref() {
        Some(wd) if !wd.is_empty() => {
            // Sanitize: remove leading slashes, filter out ".."
            let sanitized: String = wd
                .trim_start_matches('/')
                .split('/')
                .filter(|s| *s != ".." && *s != ".")
                .collect::<Vec<_>>()
                .join("/");
            if sanitized.is_empty() {
                project_path.to_string()
            } else {
                format!("{project_path}/{sanitized}")
            }
        }
        _ => project_path.to_string(),
    };

    // Collect all commands to detect tool families
    let all_cmds: Vec<&str> = [
        settings.install_command.as_deref(),
        settings.build_command.as_deref(),
        settings.run_command.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();

    // Detect required tool families and package managers
    let mut families = std::collections::HashSet::new();
    let mut pkg_managers = std::collections::HashSet::new();
    for cmd in &all_cmds {
        let tool = extract_tool(cmd);
        if let Some(family) = tool_family(tool) {
            families.insert(family);
        }
        if needs_explicit_install(tool) {
            let mgr = if tool == "bunx" { "bun" } else { tool };
            pkg_managers.insert(mgr.to_string());
        }
    }

    let mut lines = Vec::new();
    lines.push("#!/bin/bash".to_string());
    lines.push("set -e".to_string());
    lines.push("export DEBIAN_FRONTEND=noninteractive".to_string());
    lines.push(String::new());

    // Seed user environment variables (persisted to /etc/environment + exported)
    lines.extend(build_env_vars_block(env_vars));

    // Install baseline tools
    lines.push("apt-get update -qq && apt-get install -y -qq git curl iproute2 nginx".to_string());
    lines.push(String::new());

    // Clone
    lines.push(format!(
        "git clone --branch {branch} {clone_url} {project_path}"
    ));
    lines.push(String::new());

    // Install runtimes
    for family in &families {
        for cmd in runtime_install_commands(*family) {
            lines.push(cmd.to_string());
        }
    }

    // Install package managers
    for mgr in &pkg_managers {
        for cmd in pkg_manager_install_commands(mgr) {
            lines.push(cmd.to_string());
        }
    }

    if !families.is_empty() || !pkg_managers.is_empty() {
        lines.push(String::new());
    }

    // Install command
    if let Some(ref cmd) = settings.install_command {
        lines.push(format!("cd {serve_path} && {cmd}"));
    }

    // Build command
    if let Some(ref cmd) = settings.build_command {
        lines.push(format!("cd {serve_path} && {cmd}"));
    }

    lines.push(String::new());

    // Run command (with nginx reverse proxy) or static nginx
    if let Some(ref cmd) = settings.run_command {
        lines.push(format!(
            "cd {serve_path} && nohup {cmd} > /tmp/app.log 2>&1 & echo $! > /etc/VERS_PID"
        ));
        lines.push(String::new());
        lines.push(nginx_reverse_proxy_section());
    } else {
        lines.push(nginx_static_section(&serve_path));
    }

    // Write status marker
    lines.push(String::new());
    lines.push("echo done > /etc/VERS_DEPLOY_STATUS".to_string());

    // Wrap in a trap so failures also write status
    let body = lines.join("\n");
    format!(
        r#"#!/bin/bash
trap 'echo "failed: $(tail -1 /tmp/vers_deploy.log 2>/dev/null || echo unknown)" > /etc/VERS_DEPLOY_STATUS' ERR
{body}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_setup_script_with_run_command() {
        let settings = DeploySettings {
            install_command: Some("pnpm install".into()),
            build_command: Some("pnpm build".into()),
            run_command: Some("pnpm start".into()),
            working_directory: None,
        };
        let empty: HashMap<String, String> = HashMap::new();
        let script = build_setup_script(
            "https://x-access-token:tok@github.com/owner/repo.git",
            "main",
            &settings,
            &empty,
        );
        assert!(script.contains("git clone --branch main"));
        assert!(script.contains("pnpm install"));
        assert!(script.contains("pnpm build"));
        assert!(script.contains("nohup pnpm start"));
        assert!(script.contains("VERS_PORT_PLACEHOLDER"));
        assert!(script.contains("echo done > /etc/VERS_DEPLOY_STATUS"));
    }

    #[test]
    fn test_build_setup_script_static() {
        let settings = DeploySettings {
            install_command: Some("npm install".into()),
            build_command: Some("npm run build".into()),
            run_command: None,
            working_directory: Some("packages/web".into()),
        };
        let empty: HashMap<String, String> = HashMap::new();
        let script = build_setup_script(
            "https://x-access-token:tok@github.com/owner/repo.git",
            "main",
            &settings,
            &empty,
        );
        assert!(script.contains("/home/user/project/packages/web"));
        assert!(script.contains("try_files"));
        assert!(!script.contains("VERS_PID"));
    }

    #[test]
    fn test_working_directory_sanitization() {
        let settings = DeploySettings {
            install_command: None,
            build_command: None,
            run_command: None,
            working_directory: Some("../../etc/passwd".into()),
        };
        let empty: HashMap<String, String> = HashMap::new();
        let script = build_setup_script("url", "main", &settings, &empty);
        assert!(script.contains("/home/user/project/etc/passwd"));
        assert!(!script.contains(".."));
    }

    #[test]
    fn test_build_setup_script_with_env_vars() {
        let settings = DeploySettings {
            install_command: Some("npm install".into()),
            build_command: None,
            run_command: Some("npm start".into()),
            working_directory: None,
        };
        let mut env_vars: HashMap<String, String> = HashMap::new();
        env_vars.insert(
            String::from("DATABASE_URL"),
            String::from("postgres://localhost/mydb"),
        );
        env_vars.insert(String::from("API_KEY"), String::from("sk-12345"));
        let script = build_setup_script(
            "https://x-access-token:tok@github.com/owner/repo.git",
            "main",
            &settings,
            &env_vars,
        );
        // Persisted to /etc/environment
        assert!(script.contains("DATABASE_URL=postgres://localhost/mydb"));
        assert!(script.contains("API_KEY=sk-12345"));
        assert!(script.contains("/etc/environment"));
        // Exported for current script
        assert!(script.contains("export DATABASE_URL='postgres://localhost/mydb'"));
        assert!(script.contains("export API_KEY='sk-12345'"));
        // Env vars appear before git clone
        let env_pos = script.find("export DATABASE_URL").unwrap();
        let clone_pos = script.find("git clone").unwrap();
        assert!(
            env_pos < clone_pos,
            "env vars should be set before git clone"
        );
    }

    #[test]
    fn test_env_var_with_single_quote() {
        let mut env_vars = HashMap::new();
        env_vars.insert(String::from("MSG"), String::from("it's a test"));
        let script = build_setup_script("url", "main", &DeploySettings::default(), &env_vars);
        // The export should escape single quotes
        assert!(script.contains("export MSG='it'\\''s a test'"));
        // /etc/environment gets the raw value (inside heredoc, no expansion)
        assert!(script.contains("MSG=it's a test"));
    }

    #[test]
    fn test_no_env_vars_block_when_empty() {
        let empty: HashMap<String, String> = HashMap::new();
        let script = build_setup_script("url", "main", &DeploySettings::default(), &empty);
        assert!(!script.contains("/etc/environment"));
        assert!(!script.contains("VERS_ENV_EOF"));
    }
}
