use clap::{ArgGroup, ArgSettings, Clap};
use googapis::{
    google::cloud::secretmanager::v1::{
        secret_manager_service_client::SecretManagerServiceClient, AccessSecretVersionRequest,
        ListSecretsRequest, SecretPayload,
    },
    CERTIFICATES,
};
use gouth::Token;
use serde_json::Value;
use std::{path::PathBuf, string::FromUtf8Error};
use thiserror::Error;
use tonic::{
    metadata::MetadataValue,
    transport::{Certificate, Channel, ClientTlsConfig},
    Request,
};

use super::{convert::decode_env_from_json, Vault, VaultConfig};

#[derive(Clap, Debug)]
#[clap(group = ArgGroup::new("google_creds"))]
pub struct GoogleConfig {
    /// Use Google Secret Manager.
    #[clap(name = "google", long = "google", requires = "project")]
    enabled: bool,

    /// [Google] The path to credentials file. Leave blank to use gouth default credentials
    /// resolution. Cannot be used with `credentials-json`.
    #[clap(
        long,
        parse(from_os_str),
        env = "GOOGLE_APPLICATION_CREDENTIALS",
        display_order = 110,
        group = "google_creds"
    )]
    credentials_file: Option<PathBuf>,

    /// [Google] The credentials JSON. Leave blank to use gouth default credentials resolution.
    /// Cannot be used with `credentials-file`.
    #[clap(
        long,
        env = "GOOGLE_APPLICATION_CREDENTIALS_JSON",
        setting = ArgSettings::HideEnvValues,
        display_order = 111,
        group = "google_creds"
    )]
    credentials_json: Option<String>,

    /// [Google] Google project to use.
    #[clap(long, env = "GOOGLE_PROJECT", display_order = 112)]
    project: Option<String>,
}

#[derive(Error, Debug)]
pub enum GoogleError {
    #[error("Tonic configuration error")]
    TonicError(#[source] tonic::transport::Error),
    #[error("Google SA configuration is invalid")]
    ConfigurationError(#[source] gouth::Error),
    #[error("cannot load secret from Secret Manager")]
    SecretManagerError(#[source] tonic::Status),
    #[error("the value is not valid UTF-8 string")]
    InvalidString(#[source] FromUtf8Error),
    #[error("the secret is empty")]
    EmptySecret,
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
    async fn to_client(&self) -> Result<SecretManagerServiceClient<Channel>> {
        let tls_config = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(CERTIFICATES))
            .domain_name("secretmanager.googleapis.com");

        let channel = Channel::from_static("https://secretmanager.googleapis.com")
            .tls_config(tls_config)
            .map_err(GoogleError::TonicError)?
            .connect()
            .await
            .map_err(GoogleError::TonicError)?;

        let token = self.to_token()?;

        let client = SecretManagerServiceClient::with_interceptor(
            channel,
            move |mut req: tonic::Request<()>| {
                let token = token
                    .header_value()
                    .map_err(|e| tonic::Status::unknown(e.to_string()))?;
                let meta = MetadataValue::from_str(&*token)
                    .map_err(|e| tonic::Status::unknown(e.to_string()))?;
                req.metadata_mut().insert("authorization", meta);
                Ok(req)
            },
        );

        Ok(client)
    }

    fn to_token(&self) -> Result<Token> {
        let token = if let Some(path) = &self.credentials_file {
            gouth::Builder::new().file(path).build()
        } else if let Some(json) = &self.credentials_json {
            gouth::Builder::new().json(json).build()
        } else {
            Token::new()
        };
        token.map_err(GoogleError::ConfigurationError)
    }
}

impl Vault for GoogleConfig {
    #[tokio::main]
    async fn download_prefixed(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
        let mut client = self.to_client().await?;
        let project = self.project.as_ref().unwrap();
        let response = client
            .list_secrets(Request::new(ListSecretsRequest {
                parent: format!("projects/{}", project),
                page_size: 25000,
                page_token: "".to_string(),
            }))
            .await
            .map_err(GoogleError::SecretManagerError)?;
        let secrets: Vec<_> = response
            .into_inner()
            .secrets
            .into_iter()
            .filter(|f| self.secret_matches(prefix, &f.name))
            .collect();
        let mut from_kv = Vec::with_capacity(secrets.len());
        for secret in secrets {
            let value = self.get_secret_full_name(&mut client, &secret.name).await?;
            let value = String::from_utf8(value.data).map_err(GoogleError::InvalidString)?;
            let name = self.strip_prefix(prefix, &secret.name).to_string();
            from_kv.push((name, value));
        }
        Ok(from_kv)
    }

    #[tokio::main]
    async fn download_json(&self, secret_name: &str) -> anyhow::Result<Vec<(String, String)>> {
        let mut client = self.to_client().await?;
        let payload = self.get_secret(&mut client, secret_name).await?;
        let value: Value = serde_json::from_slice(&payload.data)?;
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

    async fn get_secret(
        &self,
        client: &mut SecretManagerServiceClient<Channel>,
        secret_name: &str,
    ) -> Result<SecretPayload> {
        self.get_secret_full_name(
            client,
            &format!(
                "projects/{}/secrets/{}",
                self.project.as_ref().unwrap(),
                secret_name
            ),
        )
        .await
    }

    async fn get_secret_full_name(
        &self,
        client: &mut SecretManagerServiceClient<Channel>,
        name: &str,
    ) -> Result<SecretPayload> {
        let response = client
            .access_secret_version(Request::new(AccessSecretVersionRequest {
                name: format!("{}/versions/latest", name),
            }))
            .await
            .map_err(GoogleError::SecretManagerError)?;
        response
            .into_inner()
            .payload
            .ok_or(GoogleError::EmptySecret)
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
            credentials_file: None,
            credentials_json: None,
            project: Some("kvenv".to_string()),
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
            credentials_file: None,
            credentials_json: None,
            project: Some("kvenv".to_string()),
        };

        gc.strip_project("projects");
    }

    #[test]
    #[should_panic]
    fn fail_strip_project2() {
        let gc = GoogleConfig {
            enabled: true,
            credentials_file: None,
            credentials_json: None,
            project: Some("kvenv".to_string()),
        };

        gc.strip_project("");
    }

    #[test]
    fn secret_matches_correctly() {
        let gc = GoogleConfig {
            enabled: true,
            credentials_file: None,
            credentials_json: None,
            project: Some("kvenv".to_string()),
        };

        assert!(gc.secret_matches("prefix", "projects/kvenv/secrets/prefix-1"));
        assert!(gc.secret_matches("prefix", "projects/kvenv/secrets/prefix1"));
        assert!(!gc.secret_matches("prefix", "projects/kvenv/secrets/prefi"));
    }

    #[test]
    fn strips_prefix_correctly() {
        let gc = GoogleConfig {
            enabled: true,
            credentials_file: None,
            credentials_json: None,
            project: Some("kvenv".to_string()),
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
            credentials_file: None,
            credentials_json: Some(
                env_var("GOOGLE_APPLICATION_CREDENTIALS_JSON")
                    .unwrap()
                    .into(),
            ),
            project: Some(env_var("GOOGLE_PROJECT").unwrap()),
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
            credentials_file: None,
            credentials_json: Some(
                env_var("GOOGLE_APPLICATION_CREDENTIALS_JSON")
                    .unwrap()
                    .into(),
            ),
            project: Some(env_var("GOOGLE_PROJECT").unwrap()),
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
