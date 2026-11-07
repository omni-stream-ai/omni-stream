use super::Settings;
use serde::Deserialize;

const BUNDLED: &str = include_str!(concat!(env!("OUT_DIR"), "/fcm-service-account.json"));

#[derive(Deserialize)]
pub struct Credentials {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
}

fn parse(json: &[u8], project: Option<&str>) -> Result<Credentials, String> {
    let mut credentials: Credentials =
        serde_json::from_slice(json).map_err(|_| "invalid credentials")?;
    credentials.private_key = credentials.private_key.replace("\\n", "\n");
    if project.is_some_and(|value| value != credentials.project_id)
        || credentials.project_id.is_empty()
        || !credentials
            .project_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || credentials.client_email.trim().is_empty()
        || jsonwebtoken::EncodingKey::from_rsa_pem(credentials.private_key.as_bytes()).is_err()
    {
        return Err("invalid credentials".into());
    }
    Ok(credentials)
}

// Explicit deployment settings override the shared sender embedded by release builds.
// An invalid override fails instead of silently using another sending identity.
pub fn load(settings: &Settings) -> Result<Credentials, String> {
    let bytes = if let Some(path) = &settings.credentials_file {
        std::fs::read(path).map_err(|_| "missing credentials")?
    } else if let Ok(json) = std::env::var("OMNISTREAM_FCM_SERVICE_ACCOUNT_JSON") {
        json.into_bytes()
    } else if let Ok(path) = std::env::var("OMNISTREAM_FCM_SERVICE_ACCOUNT_PATH") {
        std::fs::read(path).map_err(|_| "missing credentials")?
    } else {
        BUNDLED.as_bytes().to_vec()
    };
    if bytes.is_empty() {
        return Err("missing credentials".into());
    }
    parse(&bytes, settings.firebase_project.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_invalid_explicit_credentials_never_fall_back_to_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("account.json");
        let settings = Settings {
            credentials_file: Some(file.clone()),
            ..Settings::default()
        };
        assert_eq!(
            load(&settings).err().as_deref(),
            Some("missing credentials")
        );
        std::fs::write(file, br#"{"private_key":"private-invalid-key"}"#).unwrap();
        let error = load(&settings).err().unwrap();
        assert_eq!(error, "invalid credentials");
        assert!(!error.contains("private-invalid-key"));
    }

    #[test]
    fn bundled_credentials_are_usable_without_deployment_configuration() {
        if BUNDLED.is_empty() {
            return;
        }
        let credentials = parse(BUNDLED.as_bytes(), None).unwrap();
        assert!(!credentials.project_id.is_empty());
        assert!(parse(BUNDLED.as_bytes(), Some("another-project")).is_err());
    }
}
