use std::path::PathBuf;

#[derive(Clone)]
pub struct EmailConfig {
    // disabled | ses | smtp | file
    pub mode: String,
    pub from: Option<String>,

    // SMTP settings (mode=smtp).
    pub smtp_host: Option<String>,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,

    // File outbox for development/testing (mode=file).
    pub file_dir: PathBuf,
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            mode: "disabled".to_string(),
            from: None,
            smtp_host: None,
            smtp_port: 587,
            smtp_username: String::new(),
            smtp_password: String::new(),
            file_dir: "/tmp/slopmud_email_outbox".into(),
        }
    }
}

impl std::fmt::Debug for EmailConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmailConfig")
            .field("mode", &self.mode)
            .field("from", &self.from)
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_username", &self.smtp_username)
            .field("smtp_password", &"<redacted>")
            .field("file_dir", &self.file_dir)
            .finish()
    }
}
