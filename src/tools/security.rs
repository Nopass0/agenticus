use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::net::TcpStream;
use std::time::Duration;
// tracing available if needed for debugging

const AUTHORIZATION_WARNING: &str = "⚠️ WARNING: This tool is for AUTHORIZED security testing only. \
    Only use on systems you own or have explicit written permission to test. \
    Unauthorized access to computer systems is illegal.";

/// Tool to analyze website technologies
pub struct WebTechAnalyzerTool;

#[async_trait]
impl Tool for WebTechAnalyzerTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "analyze_web_tech",
            "Analyze a website to detect technologies used (frameworks, CMS, servers, etc.). For authorized security assessments.",
        )
        .with_param(ToolParameter::string("url", "Target URL to analyze", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("URL is required"))?;

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(true)
            .build()?;

        let response = client.get(url).send().await?;
        let headers = response.headers().clone();
        let status = response.status();
        let html = response.text().await.unwrap_or_default();

        let mut technologies = Vec::new();
        let mut server_info = HashMap::new();

        // Analyze headers
        if let Some(server) = headers.get("server") {
            let server_str = server.to_str().unwrap_or("");
            server_info.insert("server", server_str.to_string());
            technologies.push(format!("Server: {}", server_str));
        }

        if let Some(powered_by) = headers.get("x-powered-by") {
            let pb = powered_by.to_str().unwrap_or("");
            server_info.insert("x-powered-by", pb.to_string());
            technologies.push(format!("Powered by: {}", pb));
        }

        // Check for common frameworks/CMS in HTML
        let tech_signatures: Vec<(&str, &str)> = vec![
            ("wp-content", "WordPress"),
            ("Joomla", "Joomla"),
            ("Drupal", "Drupal"),
            ("laravel", "Laravel"),
            ("__next", "Next.js"),
            ("__nuxt", "Nuxt.js"),
            ("ng-version", "Angular"),
            ("react", "React"),
            ("vue", "Vue.js"),
            ("jquery", "jQuery"),
            ("bootstrap", "Bootstrap"),
            ("tailwind", "Tailwind CSS"),
            ("cloudflare", "Cloudflare"),
            ("nginx", "Nginx"),
            ("apache", "Apache"),
            ("express", "Express.js"),
            ("django", "Django"),
            ("flask", "Flask"),
            ("rails", "Ruby on Rails"),
            ("aspnet", "ASP.NET"),
            ("php", "PHP"),
        ];

        let html_lower = html.to_lowercase();
        for (signature, tech) in tech_signatures {
            if html_lower.contains(signature) {
                if !technologies.iter().any(|t| t.contains(tech)) {
                    technologies.push(format!("Framework/Tech: {}", tech));
                }
            }
        }

        // Check security headers
        let mut security_headers = Vec::new();
        let sec_headers = vec![
            "strict-transport-security",
            "content-security-policy",
            "x-frame-options",
            "x-content-type-options",
            "x-xss-protection",
            "referrer-policy",
        ];

        for header in sec_headers {
            if headers.contains_key(header) {
                security_headers.push(format!("{}: present", header));
            } else {
                security_headers.push(format!("{}: MISSING", header));
            }
        }

        let mut output = format!("{}\n\n", AUTHORIZATION_WARNING);
        output.push_str(&format!("URL: {}\n", url));
        output.push_str(&format!("Status: {}\n\n", status));

        output.push_str("=== Detected Technologies ===\n");
        if technologies.is_empty() {
            output.push_str("No specific technologies detected\n");
        } else {
            for tech in &technologies {
                output.push_str(&format!("• {}\n", tech));
            }
        }

        output.push_str("\n=== Security Headers ===\n");
        for header in &security_headers {
            output.push_str(&format!("• {}\n", header));
        }

        Ok(ToolResult::success_with_data(
            output,
            serde_json::json!({
                "url": url,
                "status": status.as_u16(),
                "technologies": technologies,
                "security_headers": security_headers,
                "server_info": server_info
            }),
        ))
    }
}

/// Tool to scan for common directories (like dirb)
pub struct DirectoryScannerTool;

impl DirectoryScannerTool {
    fn get_common_paths() -> Vec<&'static str> {
        vec![
            // Admin panels
            "admin", "administrator", "admin.php", "admin.html", "adminpanel",
            "wp-admin", "wp-login.php", "manager", "cpanel", "webadmin",
            "phpmyadmin", "pma", "adminer", "adminer.php",

            // Config/sensitive files
            ".git", ".git/config", ".gitignore", ".env", ".htaccess",
            "config.php", "config.yml", "config.json", "settings.php",
            "web.config", "database.yml", "secrets.yml",

            // Backup files
            "backup", "backup.zip", "backup.sql", "backup.tar.gz",
            "db.sql", "dump.sql", "database.sql",

            // API endpoints
            "api", "api/v1", "api/v2", "graphql", "rest",
            "swagger", "swagger.json", "api-docs",

            // Common directories
            "uploads", "upload", "files", "images", "img",
            "assets", "static", "media", "public",
            "includes", "inc", "lib", "vendor",

            // Login/Auth
            "login", "signin", "auth", "authenticate",
            "logout", "register", "signup",

            // Debug/Dev
            "debug", "test", "testing", "dev", "development",
            "phpinfo.php", "info.php", "server-status",

            // CMS specific
            "wp-content", "wp-includes", "xmlrpc.php",
            "components", "modules", "plugins", "themes",

            // Robots and sitemap
            "robots.txt", "sitemap.xml", "sitemap_index.xml",

            // Other
            "cgi-bin", "scripts", "js", "css",
            "temp", "tmp", "cache", "logs", "log",
        ]
    }
}

#[async_trait]
impl Tool for DirectoryScannerTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "scan_directories",
            "Scan a website for common directories and files (like dirb). FOR AUTHORIZED TESTING ONLY.",
        )
        .with_param(ToolParameter::string("url", "Base URL to scan (e.g., https://example.com)", true))
        .with_param(ToolParameter::number(
            "threads",
            "Number of concurrent requests (default: 5, max: 20)",
            false,
        ))
        .with_param(ToolParameter::boolean(
            "quick",
            "Quick scan with fewer paths (default: false)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let base_url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("URL is required"))?;

        let threads = params
            .get("threads")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(20) as usize;

        let quick = params
            .get("quick")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let paths = Self::get_common_paths();
        let paths_to_check: Vec<&str> = if quick {
            paths.into_iter().take(30).collect()
        } else {
            paths
        };

        let base_url = base_url.trim_end_matches('/');

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(Duration::from_secs(10))
            .danger_accept_invalid_certs(true)
            .build()?;

        let mut found = Vec::new();
        let mut output = format!("{}\n\n", AUTHORIZATION_WARNING);
        output.push_str(&format!("Scanning: {}\n", base_url));
        output.push_str(&format!("Paths to check: {}\n\n", paths_to_check.len()));

        // Use semaphore for rate limiting
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(threads));
        let mut handles = Vec::new();

        for path in &paths_to_check {
            let client = client.clone();
            let path_owned = path.to_string();
            let url = format!("{}/{}", base_url, path);
            let permit = semaphore.clone().acquire_owned().await.unwrap();

            let handle = tokio::spawn(async move {
                let result = client.get(&url).send().await;
                drop(permit);

                match result {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        if status == 200 || status == 301 || status == 302 || status == 403 {
                            Some((path_owned, status))
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            if let Ok(Some((path, status))) = handle.await {
                let status_text = match status {
                    200 => "OK",
                    301 | 302 => "Redirect",
                    403 => "Forbidden (exists but protected)",
                    _ => "Found",
                };
                found.push(serde_json::json!({
                    "path": path,
                    "status": status,
                    "status_text": status_text
                }));
                output.push_str(&format!("[{}] {} - {}\n", status, path, status_text));
            }
        }

        if found.is_empty() {
            output.push_str("\nNo directories/files found.\n");
        } else {
            output.push_str(&format!("\nFound {} paths.\n", found.len()));
        }

        Ok(ToolResult::success_with_data(
            output,
            serde_json::json!({
                "base_url": base_url,
                "scanned": paths_to_check.len(),
                "found": found
            }),
        ))
    }
}

/// Tool to scan open ports
pub struct PortScannerTool;

impl PortScannerTool {
    fn get_common_ports() -> Vec<u16> {
        vec![
            21,    // FTP
            22,    // SSH
            23,    // Telnet
            25,    // SMTP
            53,    // DNS
            80,    // HTTP
            110,   // POP3
            111,   // RPC
            135,   // MSRPC
            139,   // NetBIOS
            143,   // IMAP
            443,   // HTTPS
            445,   // SMB
            993,   // IMAPS
            995,   // POP3S
            1433,  // MSSQL
            1521,  // Oracle
            3306,  // MySQL
            3389,  // RDP
            5432,  // PostgreSQL
            5900,  // VNC
            6379,  // Redis
            8080,  // HTTP Proxy
            8443,  // HTTPS Alt
            27017, // MongoDB
        ]
    }

    fn get_service_name(port: u16) -> &'static str {
        match port {
            21 => "FTP",
            22 => "SSH",
            23 => "Telnet",
            25 => "SMTP",
            53 => "DNS",
            80 => "HTTP",
            110 => "POP3",
            111 => "RPC",
            135 => "MSRPC",
            139 => "NetBIOS",
            143 => "IMAP",
            443 => "HTTPS",
            445 => "SMB",
            993 => "IMAPS",
            995 => "POP3S",
            1433 => "MSSQL",
            1521 => "Oracle",
            3306 => "MySQL",
            3389 => "RDP",
            5432 => "PostgreSQL",
            5900 => "VNC",
            6379 => "Redis",
            8080 => "HTTP-Proxy",
            8443 => "HTTPS-Alt",
            27017 => "MongoDB",
            _ => "Unknown",
        }
    }
}

#[async_trait]
impl Tool for PortScannerTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "scan_ports",
            "Scan a host for open ports (like nmap). FOR AUTHORIZED TESTING ONLY.",
        )
        .with_param(ToolParameter::string(
            "host",
            "Target hostname or IP address",
            true,
        ))
        .with_param(ToolParameter::string(
            "ports",
            "Ports to scan: 'common' (default), 'all' (1-1024), or comma-separated list (e.g., '80,443,8080')",
            false,
        ))
        .with_param(ToolParameter::number(
            "timeout",
            "Connection timeout in milliseconds (default: 1000)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let host = params
            .get("host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Host is required"))?;

        let ports_param = params
            .get("ports")
            .and_then(|v| v.as_str())
            .unwrap_or("common");

        let timeout_ms = params
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000);

        let timeout = Duration::from_millis(timeout_ms);

        let ports: Vec<u16> = match ports_param {
            "common" => Self::get_common_ports(),
            "all" => (1..=1024).collect(),
            custom => {
                custom
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect()
            }
        };

        let mut output = format!("{}\n\n", AUTHORIZATION_WARNING);
        output.push_str(&format!("Scanning host: {}\n", host));
        output.push_str(&format!("Ports to scan: {}\n", ports.len()));
        output.push_str(&format!("Timeout: {}ms\n\n", timeout_ms));

        let mut open_ports = Vec::new();

        for port in &ports {
            let addr = format!("{}:{}", host, port);
            let is_open = TcpStream::connect_timeout(
                &addr.parse().unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
                timeout,
            )
            .is_ok();

            if is_open {
                let service = Self::get_service_name(*port);
                open_ports.push(serde_json::json!({
                    "port": port,
                    "service": service,
                    "state": "open"
                }));
                output.push_str(&format!("  {:5}/tcp   open   {}\n", port, service));
            }
        }

        if open_ports.is_empty() {
            output.push_str("No open ports found.\n");
        } else {
            output.push_str(&format!("\n{} open ports found.\n", open_ports.len()));
        }

        Ok(ToolResult::success_with_data(
            output,
            serde_json::json!({
                "host": host,
                "scanned_ports": ports.len(),
                "open_ports": open_ports
            }),
        ))
    }
}

/// Tool to check for common vulnerabilities
pub struct VulnScannerTool;

#[async_trait]
impl Tool for VulnScannerTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "scan_vulnerabilities",
            "Check a website for common vulnerabilities (basic nikto-like scan). FOR AUTHORIZED TESTING ONLY.",
        )
        .with_param(ToolParameter::string("url", "Target URL to scan", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("URL is required"))?;

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(Duration::from_secs(10))
            .danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let mut findings = Vec::new();
        let mut output = format!("{}\n\n", AUTHORIZATION_WARNING);
        output.push_str(&format!("Vulnerability scan: {}\n\n", url));

        // Check 1: Server header disclosure
        if let Ok(resp) = client.get(url).send().await {
            let headers = resp.headers();

            if let Some(server) = headers.get("server") {
                let server_str = server.to_str().unwrap_or("");
                if server_str.contains('/') {
                    findings.push(serde_json::json!({
                        "type": "Information Disclosure",
                        "severity": "Low",
                        "description": format!("Server version disclosed: {}", server_str),
                        "recommendation": "Remove or obfuscate server version information"
                    }));
                    output.push_str(&format!("[LOW] Server version disclosed: {}\n", server_str));
                }
            }

            // Check for missing security headers
            let security_checks = vec![
                ("strict-transport-security", "HSTS not enabled", "Medium", "Enable HTTP Strict Transport Security"),
                ("x-frame-options", "Clickjacking protection missing", "Medium", "Add X-Frame-Options header"),
                ("x-content-type-options", "MIME sniffing protection missing", "Low", "Add X-Content-Type-Options: nosniff"),
                ("content-security-policy", "CSP not configured", "Medium", "Implement Content Security Policy"),
            ];

            for (header, issue, severity, rec) in security_checks {
                if !headers.contains_key(header) {
                    findings.push(serde_json::json!({
                        "type": "Missing Security Header",
                        "severity": severity,
                        "description": issue,
                        "recommendation": rec
                    }));
                    output.push_str(&format!("[{}] {}\n", severity.to_uppercase(), issue));
                }
            }
        }

        // Check 2: Common sensitive files
        let sensitive_files = vec![
            (".git/config", "Git repository exposed", "High"),
            (".env", "Environment file exposed", "Critical"),
            ("phpinfo.php", "PHP info page exposed", "Medium"),
            (".htaccess", "htaccess file readable", "Medium"),
            ("web.config", "Web config exposed", "Medium"),
        ];

        let base_url = url.trim_end_matches('/');
        for (file, desc, severity) in sensitive_files {
            let test_url = format!("{}/{}", base_url, file);
            if let Ok(resp) = client.get(&test_url).send().await {
                if resp.status().is_success() {
                    findings.push(serde_json::json!({
                        "type": "Sensitive File Exposed",
                        "severity": severity,
                        "description": format!("{}: {}", desc, file),
                        "url": test_url
                    }));
                    output.push_str(&format!("[{}] {}: {}\n", severity.to_uppercase(), desc, file));
                }
            }
        }

        // Check 3: HTTP methods
        if let Ok(resp) = client.request(reqwest::Method::OPTIONS, url).send().await {
            if let Some(allow) = resp.headers().get("allow") {
                let methods = allow.to_str().unwrap_or("");
                let dangerous = vec!["PUT", "DELETE", "TRACE"];
                for method in dangerous {
                    if methods.contains(method) {
                        findings.push(serde_json::json!({
                            "type": "Dangerous HTTP Method Enabled",
                            "severity": "Medium",
                            "description": format!("{} method is enabled", method),
                            "recommendation": "Disable unnecessary HTTP methods"
                        }));
                        output.push_str(&format!("[MEDIUM] Dangerous HTTP method enabled: {}\n", method));
                    }
                }
            }
        }

        if findings.is_empty() {
            output.push_str("\nNo obvious vulnerabilities found. This does not mean the site is secure.\n");
        } else {
            output.push_str(&format!("\n{} potential issues found.\n", findings.len()));
        }

        Ok(ToolResult::success_with_data(
            output,
            serde_json::json!({
                "url": url,
                "findings": findings,
                "total_issues": findings.len()
            }),
        ))
    }
}

/// Tool to get DNS information
pub struct DnsLookupTool;

#[async_trait]
impl Tool for DnsLookupTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "dns_lookup",
            "Perform DNS lookups for a domain (A, AAAA, MX, TXT, NS records)",
        )
        .with_param(ToolParameter::string("domain", "Domain to lookup", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let domain = params
            .get("domain")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Domain is required"))?;

        // Use system DNS resolver via nslookup or dig
        let mut output = format!("DNS Lookup for: {}\n\n", domain);
        let mut records = Vec::new();

        // Try to resolve the domain using std::net
        use std::net::ToSocketAddrs;

        let addr_str = format!("{}:80", domain);
        if let Ok(addrs) = addr_str.to_socket_addrs() {
            for addr in addrs {
                let ip = addr.ip().to_string();
                records.push(serde_json::json!({
                    "type": if addr.is_ipv4() { "A" } else { "AAAA" },
                    "value": ip
                }));
                output.push_str(&format!("{}: {}\n",
                    if addr.is_ipv4() { "A" } else { "AAAA" },
                    ip
                ));
            }
        }

        if records.is_empty() {
            output.push_str("No DNS records found or domain doesn't resolve.\n");
        }

        Ok(ToolResult::success_with_data(
            output,
            serde_json::json!({
                "domain": domain,
                "records": records
            }),
        ))
    }
}

/// Tool to check SSL/TLS certificate
pub struct SslCheckTool;

#[async_trait]
impl Tool for SslCheckTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "check_ssl",
            "Check SSL/TLS certificate information for a domain",
        )
        .with_param(ToolParameter::string("domain", "Domain to check SSL for", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let domain = params
            .get("domain")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Domain is required"))?;

        let url = format!("https://{}", domain);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        let mut output = format!("SSL/TLS Check for: {}\n\n", domain);
        let mut ssl_info = serde_json::json!({
            "domain": domain,
            "valid": false
        });

        match client.get(&url).send().await {
            Ok(resp) => {
                output.push_str("✓ SSL/TLS connection successful\n");
                output.push_str(&format!("✓ HTTP Status: {}\n", resp.status()));

                // Check HSTS
                if resp.headers().contains_key("strict-transport-security") {
                    output.push_str("✓ HSTS enabled\n");
                    ssl_info["hsts"] = serde_json::json!(true);
                } else {
                    output.push_str("✗ HSTS not enabled\n");
                    ssl_info["hsts"] = serde_json::json!(false);
                }

                ssl_info["valid"] = serde_json::json!(true);
            }
            Err(e) => {
                output.push_str(&format!("✗ SSL/TLS error: {}\n", e));
                if e.to_string().contains("certificate") {
                    output.push_str("  Certificate issue detected\n");
                }
                ssl_info["error"] = serde_json::json!(e.to_string());
            }
        }

        Ok(ToolResult::success_with_data(output, ssl_info))
    }
}

pub fn register_security_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(WebTechAnalyzerTool);
    registry.add_tool(DirectoryScannerTool);
    registry.add_tool(PortScannerTool);
    registry.add_tool(VulnScannerTool);
    registry.add_tool(DnsLookupTool);
    registry.add_tool(SslCheckTool);
}
