use std::{env, fs, path::PathBuf};

fn main() {
    for name in [
        "OMNISTREAM_FCM_SERVICE_ACCOUNT_JSON",
        "OMNISTREAM_FCM_SERVICE_ACCOUNT_PATH",
        "OMNISTREAM_REQUIRE_BUNDLED_FCM",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let json = match env::var("OMNISTREAM_FCM_SERVICE_ACCOUNT_JSON") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => match env::var("OMNISTREAM_FCM_SERVICE_ACCOUNT_PATH") {
            Ok(path) if !path.is_empty() => {
                println!("cargo:rerun-if-changed={path}");
                fs::read_to_string(path).expect("Cannot read bundled FCM credentials")
            }
            _ => String::new(),
        },
    };
    if json.is_empty() {
        assert!(
            env::var("OMNISTREAM_REQUIRE_BUNDLED_FCM").as_deref() != Ok("true"),
            "Release builds require OMNISTREAM_FCM_SERVICE_ACCOUNT_JSON"
        );
    } else {
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("Invalid bundled FCM credential JSON");
        assert!(
            value["type"] == "service_account"
                && ["project_id", "client_email", "private_key"]
                    .iter()
                    .all(|name| value[name].as_str().is_some_and(|v| !v.trim().is_empty())),
            "Bundled FCM credentials must contain a service account"
        );
        assert!(
            value["project_id"]
                .as_str()
                .unwrap()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                && jsonwebtoken::EncodingKey::from_rsa_pem(
                    value["private_key"]
                        .as_str()
                        .unwrap()
                        .replace("\\n", "\n")
                        .as_bytes()
                )
                .is_ok(),
            "Bundled FCM credentials contain an invalid project or RSA key"
        );
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Missing build output directory"));
    fs::write(output.join("fcm-service-account.json"), json)
        .expect("Cannot prepare bundled FCM credentials");
}
