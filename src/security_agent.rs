use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::TcpStream;
use std::time::Duration;

const AUTHORIZATION_WARNING: &str = "⚠️ WARNING: This security agent is for AUTHORIZED testing only. \
    Only use on systems you own or have explicit written permission to test. \
    Unauthorized access to computer systems is illegal.";

/// Result of a security assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAssessmentResult {
    pub target: String,
    pub scan_type: String,
    pub findings: Vec<SecurityFinding>,
    pub summary: AssessmentSummary,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub category: String,
    pub severity: Severity,
    pub title: String,
    pub description: String,
    pub evidence: Option<String>,
    pub recommendation: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "INFO"),
            Severity::Low => write!(f, "LOW"),
            Severity::Medium => write!(f, "MEDIUM"),
            Severity::High => write!(f, "HIGH"),
            Severity::Critical => write!(f, "CRITICAL"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssessmentSummary {
    pub total_findings: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub medium_count: usize,
    pub low_count: usize,
    pub info_count: usize,
    pub open_ports: Vec<u16>,
    pub technologies_detected: Vec<String>,
    pub missing_security_headers: Vec<String>,
}

/// Security sub-agent that performs comprehensive security assessments
pub struct SecurityAgent {
    timeout: Duration,
    max_concurrent: usize,
}

impl SecurityAgent {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            max_concurrent: 10,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Perform a comprehensive security assessment on a target
    pub async fn assess(&self, target: &str, scan_type: ScanType) -> Result<SecurityAssessmentResult> {
        let mut findings = Vec::new();
        let mut summary = AssessmentSummary::default();

        // Parse target
        let (host, url) = self.parse_target(target);

        match scan_type {
            ScanType::Full => {
                // Run all scans
                self.scan_ports(&host, &mut findings, &mut summary).await;
                if let Some(u) = &url {
                    self.scan_web_tech(u, &mut findings, &mut summary).await;
                    self.scan_directories(u, &mut findings, &mut summary).await;
                    self.scan_vulnerabilities(u, &mut findings, &mut summary).await;
                }
                self.check_ssl(&host, &mut findings, &mut summary).await;
            }
            ScanType::QuickWeb => {
                if let Some(u) = &url {
                    self.scan_web_tech(u, &mut findings, &mut summary).await;
                    self.scan_vulnerabilities(u, &mut findings, &mut summary).await;
                }
            }
            ScanType::PortsOnly => {
                self.scan_ports(&host, &mut findings, &mut summary).await;
            }
            ScanType::WebOnly => {
                if let Some(u) = &url {
                    self.scan_web_tech(u, &mut findings, &mut summary).await;
                    self.scan_directories(u, &mut findings, &mut summary).await;
                    self.scan_vulnerabilities(u, &mut findings, &mut summary).await;
                }
            }
        }

        // Count findings by severity
        for finding in &findings {
            match finding.severity {
                Severity::Critical => summary.critical_count += 1,
                Severity::High => summary.high_count += 1,
                Severity::Medium => summary.medium_count += 1,
                Severity::Low => summary.low_count += 1,
                Severity::Info => summary.info_count += 1,
            }
        }
        summary.total_findings = findings.len();

        // Generate recommendations
        let recommendations = self.generate_recommendations(&findings, &summary);

        Ok(SecurityAssessmentResult {
            target: target.to_string(),
            scan_type: format!("{:?}", scan_type),
            findings,
            summary,
            recommendations,
        })
    }

    fn parse_target(&self, target: &str) -> (String, Option<String>) {
        if target.starts_with("http://") || target.starts_with("https://") {
            let url = url::Url::parse(target).ok();
            let host = url.as_ref()
                .and_then(|u| u.host_str())
                .unwrap_or(target)
                .to_string();
            (host, Some(target.to_string()))
        } else {
            let url = format!("https://{}", target);
            (target.to_string(), Some(url))
        }
    }

    async fn scan_ports(&self, host: &str, findings: &mut Vec<SecurityFinding>, summary: &mut AssessmentSummary) {
        let common_ports = vec![
            (21, "FTP"), (22, "SSH"), (23, "Telnet"), (25, "SMTP"),
            (53, "DNS"), (80, "HTTP"), (110, "POP3"), (443, "HTTPS"),
            (445, "SMB"), (1433, "MSSQL"), (3306, "MySQL"), (3389, "RDP"),
            (5432, "PostgreSQL"), (6379, "Redis"), (8080, "HTTP-Proxy"),
            (27017, "MongoDB"),
        ];

        for (port, service) in common_ports {
            let addr = format!("{}:{}", host, port);
            if let Ok(addr) = addr.parse() {
                if TcpStream::connect_timeout(&addr, self.timeout).is_ok() {
                    summary.open_ports.push(port);

                    // Flag risky open ports
                    let (severity, risk_msg) = match port {
                        21 => (Severity::Medium, "FTP is an insecure protocol"),
                        23 => (Severity::High, "Telnet transmits data in cleartext"),
                        445 => (Severity::Medium, "SMB can be vulnerable to attacks"),
                        3389 => (Severity::Medium, "RDP exposed to internet is risky"),
                        6379 => (Severity::High, "Redis often lacks authentication"),
                        27017 => (Severity::High, "MongoDB exposed can lead to data breach"),
                        _ => (Severity::Info, "Port is open"),
                    };

                    findings.push(SecurityFinding {
                        category: "Network".to_string(),
                        severity,
                        title: format!("Port {} ({}) is open", port, service),
                        description: risk_msg.to_string(),
                        evidence: Some(format!("{}:{}", host, port)),
                        recommendation: if severity >= Severity::Medium {
                            Some(format!("Consider restricting access to port {} or disabling {}", port, service))
                        } else {
                            None
                        },
                    });
                }
            }
        }
    }

    async fn scan_web_tech(&self, url: &str, findings: &mut Vec<SecurityFinding>, summary: &mut AssessmentSummary) {
        let client = match reqwest::Client::builder()
            .timeout(self.timeout)
            .danger_accept_invalid_certs(true)
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        if let Ok(resp) = client.get(url).send().await {
            let headers = resp.headers().clone();
            let html = resp.text().await.unwrap_or_default();

            // Detect technologies
            let tech_patterns = vec![
                ("WordPress", vec!["wp-content", "wp-includes"]),
                ("Joomla", vec!["Joomla", "/media/jui/"]),
                ("Drupal", vec!["Drupal", "sites/default"]),
                ("Laravel", vec!["laravel", "csrf-token"]),
                ("React", vec!["react", "_reactRoot"]),
                ("Vue.js", vec!["vue", "__VUE__"]),
                ("Angular", vec!["ng-version", "ng-app"]),
                ("Next.js", vec!["__next", "_next/static"]),
                ("jQuery", vec!["jquery"]),
                ("Bootstrap", vec!["bootstrap"]),
            ];

            for (tech, patterns) in tech_patterns {
                if patterns.iter().any(|p| html.to_lowercase().contains(&p.to_lowercase())) {
                    summary.technologies_detected.push(tech.to_string());
                    findings.push(SecurityFinding {
                        category: "Technology".to_string(),
                        severity: Severity::Info,
                        title: format!("{} detected", tech),
                        description: format!("The website appears to use {}", tech),
                        evidence: None,
                        recommendation: None,
                    });
                }
            }

            // Check server header
            if let Some(server) = headers.get("server") {
                let server_str = server.to_str().unwrap_or("");
                if server_str.contains('/') {
                    findings.push(SecurityFinding {
                        category: "Information Disclosure".to_string(),
                        severity: Severity::Low,
                        title: "Server version disclosed".to_string(),
                        description: format!("Server header reveals: {}", server_str),
                        evidence: Some(server_str.to_string()),
                        recommendation: Some("Configure server to hide version information".to_string()),
                    });
                }
            }

            // Check security headers
            let security_headers = vec![
                ("strict-transport-security", "HSTS", Severity::Medium),
                ("content-security-policy", "CSP", Severity::Medium),
                ("x-frame-options", "Clickjacking Protection", Severity::Medium),
                ("x-content-type-options", "MIME Sniffing Protection", Severity::Low),
                ("x-xss-protection", "XSS Protection", Severity::Low),
                ("referrer-policy", "Referrer Policy", Severity::Low),
            ];

            for (header, name, severity) in security_headers {
                if !headers.contains_key(header) {
                    summary.missing_security_headers.push(header.to_string());
                    findings.push(SecurityFinding {
                        category: "Security Headers".to_string(),
                        severity,
                        title: format!("Missing {} header", name),
                        description: format!("The {} header is not set", header),
                        evidence: None,
                        recommendation: Some(format!("Add the {} header to improve security", header)),
                    });
                }
            }
        }
    }

    async fn scan_directories(&self, url: &str, findings: &mut Vec<SecurityFinding>, _summary: &mut AssessmentSummary) {
        let client = match reqwest::Client::builder()
            .timeout(self.timeout)
            .danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        let sensitive_paths = vec![
            (".git/config", "Git repository exposed", Severity::Critical),
            (".env", "Environment file exposed", Severity::Critical),
            ("phpinfo.php", "PHP info page exposed", Severity::Medium),
            (".htaccess", "htaccess file readable", Severity::Medium),
            ("wp-config.php.bak", "WordPress config backup", Severity::Critical),
            ("backup.sql", "Database backup exposed", Severity::Critical),
            ("admin", "Admin panel found", Severity::Info),
            ("phpmyadmin", "phpMyAdmin found", Severity::Medium),
            (".DS_Store", "macOS metadata file exposed", Severity::Low),
            ("crossdomain.xml", "Flash crossdomain policy", Severity::Low),
        ];

        let base_url = url.trim_end_matches('/');

        for (path, desc, severity) in sensitive_paths {
            let test_url = format!("{}/{}", base_url, path);
            if let Ok(resp) = client.get(&test_url).send().await {
                let status = resp.status().as_u16();
                if status == 200 {
                    findings.push(SecurityFinding {
                        category: "Sensitive File".to_string(),
                        severity,
                        title: desc.to_string(),
                        description: format!("Found accessible: {}", path),
                        evidence: Some(test_url),
                        recommendation: Some("Remove or restrict access to this file".to_string()),
                    });
                } else if status == 403 {
                    findings.push(SecurityFinding {
                        category: "Directory".to_string(),
                        severity: Severity::Info,
                        title: format!("{} exists but is protected", path),
                        description: "The resource exists but access is denied".to_string(),
                        evidence: Some(test_url),
                        recommendation: None,
                    });
                }
            }
        }
    }

    async fn scan_vulnerabilities(&self, url: &str, findings: &mut Vec<SecurityFinding>, _summary: &mut AssessmentSummary) {
        let client = match reqwest::Client::builder()
            .timeout(self.timeout)
            .danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        // Check HTTP methods
        if let Ok(resp) = client.request(reqwest::Method::OPTIONS, url).send().await {
            if let Some(allow) = resp.headers().get("allow") {
                let methods = allow.to_str().unwrap_or("");
                for dangerous in ["PUT", "DELETE", "TRACE"] {
                    if methods.contains(dangerous) {
                        findings.push(SecurityFinding {
                            category: "HTTP Methods".to_string(),
                            severity: Severity::Medium,
                            title: format!("{} method enabled", dangerous),
                            description: format!("The {} HTTP method is enabled", dangerous),
                            evidence: Some(methods.to_string()),
                            recommendation: Some("Disable unnecessary HTTP methods".to_string()),
                        });
                    }
                }
            }
        }

        // Check for CORS misconfiguration
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Origin", "https://evil.com".parse().unwrap());

        if let Ok(resp) = client.get(url).headers(headers).send().await {
            if let Some(acao) = resp.headers().get("access-control-allow-origin") {
                let acao_str = acao.to_str().unwrap_or("");
                if acao_str == "*" || acao_str == "https://evil.com" {
                    findings.push(SecurityFinding {
                        category: "CORS".to_string(),
                        severity: Severity::Medium,
                        title: "Permissive CORS configuration".to_string(),
                        description: format!("CORS allows origin: {}", acao_str),
                        evidence: Some(acao_str.to_string()),
                        recommendation: Some("Restrict CORS to trusted origins only".to_string()),
                    });
                }
            }
        }
    }

    async fn check_ssl(&self, host: &str, findings: &mut Vec<SecurityFinding>, _summary: &mut AssessmentSummary) {
        let client = match reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        let url = format!("https://{}", host);

        match client.get(&url).send().await {
            Ok(resp) => {
                findings.push(SecurityFinding {
                    category: "SSL/TLS".to_string(),
                    severity: Severity::Info,
                    title: "SSL/TLS certificate valid".to_string(),
                    description: "HTTPS connection successful with valid certificate".to_string(),
                    evidence: None,
                    recommendation: None,
                });

                // Check HSTS
                if !resp.headers().contains_key("strict-transport-security") {
                    findings.push(SecurityFinding {
                        category: "SSL/TLS".to_string(),
                        severity: Severity::Medium,
                        title: "HSTS not enabled".to_string(),
                        description: "HTTP Strict Transport Security is not configured".to_string(),
                        evidence: None,
                        recommendation: Some("Enable HSTS to prevent downgrade attacks".to_string()),
                    });
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("certificate") {
                    findings.push(SecurityFinding {
                        category: "SSL/TLS".to_string(),
                        severity: Severity::High,
                        title: "SSL certificate issue".to_string(),
                        description: format!("Certificate error: {}", err_str),
                        evidence: Some(err_str),
                        recommendation: Some("Fix or renew SSL certificate".to_string()),
                    });
                }
            }
        }
    }

    fn generate_recommendations(&self, findings: &[SecurityFinding], summary: &AssessmentSummary) -> Vec<String> {
        let mut recommendations = Vec::new();

        if summary.critical_count > 0 {
            recommendations.push("🚨 CRITICAL: Address critical findings immediately - they may allow unauthorized access.".to_string());
        }

        if summary.high_count > 0 {
            recommendations.push("⚠️ HIGH: Fix high-severity issues as soon as possible.".to_string());
        }

        if !summary.missing_security_headers.is_empty() {
            recommendations.push(format!(
                "📋 Add missing security headers: {}",
                summary.missing_security_headers.join(", ")
            ));
        }

        // Check for specific risky ports
        for port in &summary.open_ports {
            match port {
                23 => recommendations.push("🔌 Disable Telnet and use SSH instead.".to_string()),
                6379 => recommendations.push("🔌 Configure Redis authentication and firewall rules.".to_string()),
                27017 => recommendations.push("🔌 Enable MongoDB authentication and restrict network access.".to_string()),
                _ => {}
            }
        }

        // Generic recommendations
        if findings.iter().any(|f| f.category == "Sensitive File") {
            recommendations.push("📁 Remove or restrict access to sensitive files.".to_string());
        }

        if recommendations.is_empty() {
            recommendations.push("✅ No critical issues found. Continue with regular security assessments.".to_string());
        }

        recommendations
    }

    /// Generate a formatted report
    pub fn format_report(&self, result: &SecurityAssessmentResult) -> String {
        let mut report = String::new();

        report.push_str(&format!("{}\n\n", AUTHORIZATION_WARNING));
        report.push_str("═══════════════════════════════════════════════════════════\n");
        report.push_str("                    SECURITY ASSESSMENT REPORT              \n");
        report.push_str("═══════════════════════════════════════════════════════════\n\n");

        report.push_str(&format!("Target: {}\n", result.target));
        report.push_str(&format!("Scan Type: {}\n\n", result.scan_type));

        // Summary
        report.push_str("───────────────────────────────────────────────────────────\n");
        report.push_str("                         SUMMARY                           \n");
        report.push_str("───────────────────────────────────────────────────────────\n");
        report.push_str(&format!("Total Findings: {}\n", result.summary.total_findings));
        report.push_str(&format!("  🔴 Critical: {}\n", result.summary.critical_count));
        report.push_str(&format!("  🟠 High: {}\n", result.summary.high_count));
        report.push_str(&format!("  🟡 Medium: {}\n", result.summary.medium_count));
        report.push_str(&format!("  🟢 Low: {}\n", result.summary.low_count));
        report.push_str(&format!("  ⚪ Info: {}\n\n", result.summary.info_count));

        if !result.summary.open_ports.is_empty() {
            report.push_str(&format!("Open Ports: {:?}\n", result.summary.open_ports));
        }
        if !result.summary.technologies_detected.is_empty() {
            report.push_str(&format!("Technologies: {}\n", result.summary.technologies_detected.join(", ")));
        }
        report.push('\n');

        // Findings by severity
        report.push_str("───────────────────────────────────────────────────────────\n");
        report.push_str("                        FINDINGS                           \n");
        report.push_str("───────────────────────────────────────────────────────────\n");

        let mut sorted_findings = result.findings.clone();
        sorted_findings.sort_by(|a, b| b.severity.cmp(&a.severity));

        for finding in sorted_findings {
            let severity_icon = match finding.severity {
                Severity::Critical => "🔴",
                Severity::High => "🟠",
                Severity::Medium => "🟡",
                Severity::Low => "🟢",
                Severity::Info => "⚪",
            };

            report.push_str(&format!("\n{} [{}] {}\n", severity_icon, finding.severity, finding.title));
            report.push_str(&format!("   Category: {}\n", finding.category));
            report.push_str(&format!("   Description: {}\n", finding.description));
            if let Some(evidence) = &finding.evidence {
                report.push_str(&format!("   Evidence: {}\n", evidence));
            }
            if let Some(rec) = &finding.recommendation {
                report.push_str(&format!("   → {}\n", rec));
            }
        }

        // Recommendations
        report.push_str("\n───────────────────────────────────────────────────────────\n");
        report.push_str("                     RECOMMENDATIONS                        \n");
        report.push_str("───────────────────────────────────────────────────────────\n\n");

        for rec in &result.recommendations {
            report.push_str(&format!("• {}\n", rec));
        }

        report.push_str("\n═══════════════════════════════════════════════════════════\n");
        report.push_str("                      END OF REPORT                         \n");
        report.push_str("═══════════════════════════════════════════════════════════\n");

        report
    }
}

impl Default for SecurityAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ScanType {
    Full,
    QuickWeb,
    PortsOnly,
    WebOnly,
}

/// Tool module for exposing security agent as a tool
pub mod tool {
    use super::*;
    use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
    use async_trait::async_trait;
    use serde_json::Value;

    pub struct InvokeSecurityAgentTool;

    #[async_trait]
    impl Tool for InvokeSecurityAgentTool {
        fn schema(&self) -> ToolSchema {
            ToolSchema::new(
                "invoke_security_agent",
                "Invoke the security sub-agent to perform a comprehensive security assessment. FOR AUTHORIZED TESTING ONLY.",
            )
            .with_param(ToolParameter::string(
                "target",
                "Target URL or hostname to assess (e.g., https://example.com or example.com)",
                true,
            ))
            .with_param(ToolParameter::string(
                "scan_type",
                "Type of scan: 'full' (all checks), 'quick_web' (fast web checks), 'ports_only', 'web_only'. Default: full",
                false,
            ))
        }

        async fn execute(&self, params: Value) -> Result<ToolResult> {
            let target = params
                .get("target")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Target is required"))?;

            let scan_type_str = params
                .get("scan_type")
                .and_then(|v| v.as_str())
                .unwrap_or("full");

            let scan_type = match scan_type_str.to_lowercase().as_str() {
                "full" => ScanType::Full,
                "quick_web" | "quick" => ScanType::QuickWeb,
                "ports_only" | "ports" => ScanType::PortsOnly,
                "web_only" | "web" => ScanType::WebOnly,
                _ => ScanType::Full,
            };

            let agent = SecurityAgent::new();
            let result = agent.assess(target, scan_type).await?;
            let report = agent.format_report(&result);

            Ok(ToolResult::success_with_data(
                report,
                serde_json::json!({
                    "target": result.target,
                    "scan_type": result.scan_type,
                    "summary": {
                        "total": result.summary.total_findings,
                        "critical": result.summary.critical_count,
                        "high": result.summary.high_count,
                        "medium": result.summary.medium_count,
                        "low": result.summary.low_count,
                        "info": result.summary.info_count,
                        "open_ports": result.summary.open_ports,
                        "technologies": result.summary.technologies_detected,
                        "missing_headers": result.summary.missing_security_headers,
                    },
                    "findings": result.findings,
                    "recommendations": result.recommendations,
                }),
            ))
        }
    }

    pub fn register_security_agent_tools(registry: &mut crate::tools::ToolRegistry) {
        registry.add_tool(InvokeSecurityAgentTool);
    }
}
