# Changelog

<!-- next-header -->

## Unreleased (ReleaseDate)

- AWS Secret Manager integration no longer interprets keys in prefixed mode as JSON,
- `--snapshot-env` option is not valid in `cache` command only.

## 0.3.2 (2023-02-06)

- kvenv reports better errors when downloading single key (it needs to be JSON)

## 0.3.1 (2023-02-05)

- Vault is supported in the default featureset now,
- kvenv uses Rust 2021 now
- kvenv uses rustls as a TLS library instead of relying on native-tls

## 0.3.0 (2022-12-04)

- Updated to Clap 4 - this changed what & how parameters are accepted
- Added support for Hashicorp Vault

## 0.2.0 (2021-04-30)

- Added support for ASP.NET Core-compatible key format
- Removed the `cleanup` command
- `kvenv` now fails if environment in JSON secret has invalid keys
- Removed short argument options for Azure
- Added GCP Secret Manager support
- Added AWS Secrets Manager support
- Added ability to build only selected clouds
- Change the command line

## 0.1.0 (2021-01-03)

- Initial release
