//! compliance
//!
//! Shared primitives for the compliance portal + log archival.

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const LOG_LAYOUT_VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream<'a> {
    /// Human-readable logfmt of all session events.
    All,
    /// IP-based login/logout audit log.
    Login,
    /// Per-character view of the all-events log.
    Character(&'a str),
    /// User-submitted abuse reports (logfmt).
    Reports,
}

pub fn object_relpath(stream: LogStream<'_>, ts: DateTime<Utc>) -> String {
    let (y, m, d) = (ts.year(), ts.month(), ts.day());

    match stream {
        LogStream::All => format!(
            "{}/{}/{:04}/{:02}/{:02}.log",
            LOG_LAYOUT_VERSION, "all", y, m, d
        ),
        LogStream::Login => format!(
            "{}/{}/{:04}/{:02}/{:02}.log",
            LOG_LAYOUT_VERSION, "login", y, m, d
        ),
        LogStream::Character(name) => {
            let name = name.trim_matches('/');
            format!(
                "{}/{}/{}/{:04}/{:02}/{:02}.log",
                LOG_LAYOUT_VERSION, "char", name, y, m, d
            )
        }
        LogStream::Reports => format!(
            "{}/{}/{:04}/{:02}/{:02}.log",
            LOG_LAYOUT_VERSION, "reports", y, m, d
        ),
    }
}

pub fn s3_key(prefix: &str, stream: LogStream<'_>, ts: DateTime<Utc>) -> String {
    let prefix = prefix.trim_matches('/');
    let rel = object_relpath(stream, ts);
    if prefix.is_empty() {
        rel
    } else {
        format!("{prefix}/{rel}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailDomainRule {
    /// A domain suffix such as `gov`, `mil`, or `example.org`.
    pub suffix: String,
    /// Whether to show this suffix as an option in the UI.
    #[serde(default)]
    pub advertised: bool,
    /// Optional UI grouping label (e.g. country name).
    #[serde(default)]
    pub country: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompliancePortalConfig {
    /// Domain suffix allowlist for access-key emails.
    #[serde(default)]
    pub email_domain_allowlist: Vec<EmailDomainRule>,
}

impl CompliancePortalConfig {
    pub fn advertised_domain_suffixes(&self) -> Vec<String> {
        let mut out = self
            .email_domain_allowlist
            .iter()
            .filter(|r| r.advertised)
            .map(|r| normalize_suffix(&r.suffix))
            .collect::<Vec<_>>();
        out.sort();
        out.dedup();
        out
    }

    pub fn advertised_domain_suffixes_by_country(&self) -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for r in &self.email_domain_allowlist {
            if !r.advertised {
                continue;
            }
            let country = r.country.as_deref().unwrap_or("Other").trim();
            let country = if country.is_empty() { "Other" } else { country };
            out.entry(country.to_string())
                .or_default()
                .push(normalize_suffix(&r.suffix));
        }

        for v in out.values_mut() {
            v.sort();
            v.dedup();
        }

        out
    }

    pub fn email_allowed(&self, email: &str) -> bool {
        email_allowed_by_rules(email, &self.email_domain_allowlist)
    }
}

pub fn email_allowed_by_rules(email: &str, rules: &[EmailDomainRule]) -> bool {
    let email = email.trim();
    let Some((_, domain)) = email.rsplit_once('@') else {
        return false;
    };

    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() {
        return false;
    }

    for r in rules {
        let suf = normalize_suffix(&r.suffix);
        if suf.is_empty() {
            continue;
        }
        if domain == suf {
            return true;
        }
        // Require a label boundary so `notexample.org` does not match `example.org`.
        if domain.ends_with(&format!(".{suf}")) {
            return true;
        }
    }

    false
}

fn normalize_suffix(s: &str) -> String {
    s.trim()
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_allowed_suffix_boundary() {
        let rules = vec![
            EmailDomainRule {
                suffix: "gov".to_string(),
                advertised: true,
                country: None,
            },
            EmailDomainRule {
                suffix: "example.org".to_string(),
                advertised: false,
                country: None,
            },
        ];

        assert!(email_allowed_by_rules("a@agency.gov", &rules));
        assert!(!email_allowed_by_rules("a@agency.gov.evil", &rules));

        assert!(email_allowed_by_rules("a@example.org", &rules));
        assert!(email_allowed_by_rules("a@sub.example.org", &rules));
        assert!(!email_allowed_by_rules("a@notexample.org", &rules));
    }
}
