use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use clap::{arg, ArgGroup, Args};
use google_secretmanager1::{
    hyper, hyper::client::HttpConnector, hyper_rustls, hyper_rustls::HttpsConnector, oauth2,
};
use serde_json::Value;
use std::path::PathBuf;
use thiserror::Error;

use super::{convert::decode_env_from_json, Vault, VaultConfig};

type SecretManager = google_secretmanager1::SecretManager<HttpsConnector<HttpConnector>>;

#[derive(Args, Debug)]
#[command(group = ArgGroup::new("google_creds"))]
pub struct GoogleConfig {
    /// Use Google Secret Manager.
    #[arg(
        name = "google",
        long = "google",
        group = "cloud",
        requires = "google_project",
        display_order = 300
    )]
    enabled: bool,

    /// [Google] The path to credentials file. Leave blank to use efault credentials
    /// resolution. Cannot be used with `credentials-json`.
    #[arg(
        long,
        value_parser,
        env = "GOOGLE_APPLICATION_CREDENTIALS",
        display_order = 301,
        group = "google_creds"
    )]
    google_credentials_file: Option<PathBuf>,

    /// [Google] The credentials JSON. Leave blank to use default credentials resolution.
    /// Cannot be used with `credentials-file`.
    #[arg(
        long,
        env = "GOOGLE_APPLICATION_CREDENTIALS_JSON",
        hide_env_values = true,
        display_order = 302,
        group = "google_creds"
    )]
    google_credentials_json: Option<String>,

    /// [Google] Google project to use.
    #[clap(long, env = "GOOGLE_PROJECT", display_order = 303)]
    google_project: Option<String>,
}

#[derive(Error, Debug)]
pub enum GoogleError {
    #[error("Google SA configuration is invalid")]
    ConfigurationError(#[source] std::io::Error),
    #[error("secret manager operation failed")]
    SecretManagerError(#[source] google_secretmanager1::Error),
    #[error("the secret is empty")]
    EmptySecret,
    #[error("there are no secrets in the project")]
    NoSecrets,
    #[error("secret encoding is invalid")]
    WrongEncoding(#[source] anyhow::Error),
    #[error("cannot decode secret - it is not a valid JSON")]
    DecodeError(#[source] serde_json::Error),
}

pub type Result<T, E = GoogleError> = std::result::Result<T, E>;

impl VaultConfig for GoogleConfig {
    type Vault = GoogleConfig;

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn into_vault(self) -> anyhow::Result<Self::Vault> {
        Ok(self)
    }
}

impl GoogleConfig {
    async fn to_manager(&self) -> Result<SecretManager> {
        let auth = self
            .to_authenticator()
            .await
            .map_err(GoogleError::ConfigurationError)?;
        let manager = SecretManager::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .https_or_http()
                    .enable_http1()
                    .enable_http2()
                    .build(),
            ),
            auth,
        );
        Ok(manager)
    }

    async fn to_authenticator(
        &self,
    ) -> std::io::Result<oauth2::authenticator::Authenticator<HttpsConnector<HttpConnector>>> {
        if let Some(path) = &self.google_credentials_file {
            let key = oauth2::read_service_account_key(path).await?;
            let auth = oauth2::ServiceAccountAuthenticator::builder(key)
                .build()
                .await?;
            Ok(auth)
        } else if let Some(json) = &self.google_credentials_json {
            let key = oauth2::parse_service_account_key(json)?;
            let auth = oauth2::ServiceAccountAuthenticator::builder(key)
                .build()
                .await?;
            Ok(auth)
        } else {
            let opts = oauth2::ApplicationDefaultCredentialsFlowOpts::default();
            let auth = match oauth2::ApplicationDefaultCredentialsAuthenticator::builder(opts).await
            {
                oauth2::authenticator::ApplicationDefaultCredentialsTypes::ServiceAccount(auth) => {
                    auth.build().await?
                }
                oauth2::authenticator::ApplicationDefaultCredentialsTypes::InstanceMetadata(
                    auth,
                ) => auth.build().await?,
            };
            Ok(auth)
        }
    }
}

impl Vault for GoogleConfig {
    #[tokio::main]
    async fn download_prefixed(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
        let mut manager = self.to_manager().await?;
        let project = self.google_project.as_ref().unwrap();
        let response = manager
            .projects()
            .secrets_list(&format!("projects/{project}"))
            .page_size(250)
            .doit()
            .await
            .map_err(GoogleError::SecretManagerError)?;
        let secrets: Vec<_> = response
            .1
            .secrets
            .ok_or(GoogleError::NoSecrets)?
            .into_iter()
            .filter(|f| f.name.is_some())
            .filter(|f| self.secret_matches(prefix, f.name.as_ref().unwrap()))
            .collect();
        let mut from_kv = Vec::with_capacity(secrets.len());
        for secret in secrets {
            let value = self
                .get_secret_full_name(&mut manager, secret.name.as_ref().unwrap())
                .await?;
            let name = self
                .strip_prefix(prefix, secret.name.as_ref().unwrap())
                .to_string();
            from_kv.push((name, value));
        }
        Ok(from_kv)
    }

    #[tokio::main]
    async fn download_json(&self, secret_name: &str) -> anyhow::Result<Vec<(String, String)>> {
        let mut manager = self.to_manager().await?;
        let secret = self.get_secret(&mut manager, secret_name).await?;
        let value: Value = serde_json::from_str(&secret).map_err(GoogleError::DecodeError)?;
        decode_env_from_json(secret_name, value)
    }
}

impl GoogleConfig {
    fn strip_project<'a>(&'_ self, name: &'a str) -> &'a str {
        let idx = name.rfind('/').unwrap();
        &name[(idx + 1)..]
    }

    fn secret_matches(&self, prefix: &str, name: &str) -> bool {
        self.strip_project(name).starts_with(prefix)
    }

    fn strip_prefix<'a>(&'_ self, prefix: &'_ str, name: &'a str) -> &'a str {
        &self.strip_project(name)[prefix.len()..]
    }

    async fn get_secret(&self, client: &mut SecretManager, secret_name: &str) -> Result<String> {
        self.get_secret_full_name(
            client,
            &format!(
                "projects/{}/secrets/{}",
                self.google_project.as_ref().unwrap(),
                secret_name
            ),
        )
        .await
    }

    async fn get_secret_full_name(
        &self,
        manager: &mut SecretManager,
        name: &str,
    ) -> Result<String> {
        let data = manager
            .projects()
            .secrets_versions_access(&format!("{name}/versions/latest"))
            .doit()
            .await
            .map_err(GoogleError::SecretManagerError)?
            .1
            .payload
            .ok_or(GoogleError::EmptySecret)?
            .data
            .ok_or(GoogleError::EmptySecret)?;
        let raw_bytes = base64
            .decode(data)
            .map_err(|e| GoogleError::WrongEncoding(anyhow::anyhow!(e)))?;
        String::from_utf8(raw_bytes).map_err(|e| GoogleError::WrongEncoding(anyhow::anyhow!(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "integration-tests")]
    macro_rules! env {
        ($a:expr) => {
            $a.to_string()
        };
        ($a:expr, $b:expr) => {
            ($a.to_string(), $b.to_string())
        };
    }

    #[test]
    fn can_strip_project() {
        let gc = GoogleConfig {
            enabled: true,
            google_credentials_file: None,
            google_credentials_json: None,
            google_project: Some("kvenv".to_string()),
        };

        assert_eq!(
            "thisisit",
            gc.strip_project("projects/kvenv/secrets/thisisit")
        );
        assert_eq!(
            "thisisit",
            gc.strip_project("projects/kve/secrets/thisisit")
        );
    }

    #[test]
    #[should_panic]
    fn fail_strip_project1() {
        let gc = GoogleConfig {
            enabled: true,
            google_credentials_file: None,
            google_credentials_json: None,
            google_project: Some("kvenv".to_string()),
        };

        gc.strip_project("projects");
    }

    #[test]
    #[should_panic]
    fn fail_strip_project2() {
        let gc = GoogleConfig {
            enabled: true,
            google_credentials_file: None,
            google_credentials_json: None,
            google_project: Some("kvenv".to_string()),
        };

        gc.strip_project("");
    }

    #[test]
    fn secret_matches_correctly() {
        let gc = GoogleConfig {
            enabled: true,
            google_credentials_file: None,
            google_credentials_json: None,
            google_project: Some("kvenv".to_string()),
        };

        assert!(gc.secret_matches("prefix", "projects/kvenv/secrets/prefix-1"));
        assert!(gc.secret_matches("prefix", "projects/kvenv/secrets/prefix1"));
        assert!(!gc.secret_matches("prefix", "projects/kvenv/secrets/prefi"));
    }

    #[test]
    fn strips_prefix_correctly() {
        let gc = GoogleConfig {
            enabled: true,
            google_credentials_file: None,
            google_credentials_json: None,
            google_project: Some("kvenv".to_string()),
        };

        assert_eq!(
            "-1",
            gc.strip_prefix("prefix", "project/kvenv/secrets/prefix-1")
        );
        assert_eq!(
            "1",
            gc.strip_prefix("prefix", "project/kvenv/secrets/prefix1")
        );
        assert_eq!(
            "ENV_NAME",
            gc.strip_prefix("prefix", "project/kvenv/secrets/prefixENV_NAME")
        );
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn integration_tests_single_value() {
        use std::env::var as env_var;
        let cfg = GoogleConfig {
            enabled: true,
            google_credentials_file: None,
            google_credentials_json: Some(env_var("GOOGLE_APPLICATION_CREDENTIALS_JSON").unwrap()),
            google_project: Some(env_var("GOOGLE_PROJECT").unwrap()),
        };
        let proc_env = cfg
            .into_vault()
            .unwrap()
            .download_json("integ-tests")
            .unwrap();
        assert_eq!(vec![env!("INTEGRATION_TESTS", "work")], proc_env);
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn integration_tests_prefixed() {
        use std::env::var as env_var;
        let cfg = GoogleConfig {
            enabled: true,
            google_credentials_file: None,
            google_credentials_json: Some(env_var("GOOGLE_APPLICATION_CREDENTIALS_JSON").unwrap()),
            google_project: Some(env_var("GOOGLE_PROJECT").unwrap()),
        };
        let proc_env = cfg
            .into_vault()
            .unwrap()
            .download_prefixed("prefixed-")
            .unwrap();
        assert_eq!(
            vec![
                env!("INTEGRATION_TESTS_A", "work1"),
                env!("INTEGRATION_TESTS_B", "work2"),
                env!("INTEGRATION_TESTS_C", "work3")
            ],
            proc_env
        );
    }
}
