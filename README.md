# kvenv

`kvenv` is a simple command-line utility written in Rust that allows running arbitrary command with
a custom environment that is loaded from Azure KeyVault, GCP Secret Manager, AWS Secrets Manager or
Hashicorp Vault.

The main usage is in CI/CD pipelines - if your tool of choice does not support convenient,
per-project secrets (or the functionality does not support multitenancy) management, you might store
the secrets in KV and then subsequently load it using the `kvenv` tool, without exposing secret
store credentials to the processes.

## Usage

### Basics

The app has three base commands:

* `cache` - download an environment and store it in temporary file,
* `run-with` - run the command with environment made with `cache` command, and
* `run-in` - run the command with freshly downloaded environment.

`cache` and `run-with` allow you to download the environment once, and use it for subsequent calls.
This can be used to optimize the number of network calls, sacrificing secrecy (because you store the
secret on-disk and, if not properly guarded, can be read by anyone).

`run-in` can be used to run a command without storing anything to the disk. It downloads the
environment and keeps it only in memory.

The environment downloaded from the secret storage is then joined with OS environment and pass it as
the process environment to the executed command.

### Running command in fresh environment

`run-in` can be used to start a command in an environment downloaded from the Cloud secret storage.

```sh
kvenv run-in [OPTIONS] <--secret-name <SECRET_NAME>|--secret-prefix <SECRET_PREFIX>> <--aws|--azure|--google|--vault> <COMMAND>
```

Example:

```sh
$ kvenv run-in \
    --azure
    --azure-tenant-id 00000000-0000-0000-0000-0000000000000\
    --azure-client-id 00000000-0000-0000-0000-0000000000000\
    --azure-client-secret appsecret \
    --azure-keyvault-name example-keyvault \
    --secret-name test \
    -- env
DISPLAY=:0
LANG=en_US.UTF-8
PATH=/home/user
...
KEY_FROM_KV=Test
```

### Caching environment for faster subsequent runs

`cache` + `run-with` pair can be used to first cache the environment and then run the commands with
that environment multiple times without accessing the storage at all.

Cache the environment

```sh
$ kvenv cache \
    --azure
    --azure-tenant-id 00000000-0000-0000-0000-0000000000000\
    --azure-client-id 00000000-0000-0000-0000-0000000000000\
    --azure-client-secret appsecret \
    --azure-keyvault-name example-keyvault \
    --secret-name test \
    -- env
/tmp/kvenv-xxxxx.json
```

Run a command with the environment

```sh
$ kvenv run-with --env-file /tmp/kvenv-xxxxx.json -- env
DISPLAY=:0
LANG=en_US.UTF-8
PATH=/home/user
...
KEY_FROM_KV=Test
```

Remove the cached file

```sh
$ rm /tmp/kvenv-xxxxx.json
```

#### Snapshotting

The `cache` command supports `--snapshot-env` option that will store the `kvenv` process environment
to the cached file and use it for subsequent runs instead of fresh process env.

### Cloud secret storage selection

Every command that downloads environment (`cache` and `run-in`) takes one of the supported clouds:

#### `--aws`

Use AWS Secret Manager. It expects

1. `--aws-region` - AWS region.

It uses [`rusoto`] crate underneath and supports any [AWS credentials]. You can also specify
credentials directly using:

1. `--aws-access-key-id` (or `AWS_ACCESS_KEY_ID` environment variable), and
2. `--aws-secret-access-key` (or `AWS_SECRET_ACCESS_KEY` environment variable).

#### `--azure`

Uses Azure KeyVault. It expects:

1. `--azure-keyvault-name` - the name of KeyVault, or
2. `--azure-keyvault-url` - the full URL to KeyVault.

If name is provided, it constructs the KV url using `https://{name}.vault.azure.net`.

The app uses [`azure-sdk-for-rust`], thus supports all the [Azure authentication methods]. You can
also specify credentials directly:

1. `--azure-tenant-id` (or `AZURE_TENATN_ID`),
2. `--azure-client-id` (or `AZURE_CLIENT_ID`), and
3. `--azure-client-secret` (or `AZURE_CLIENT_SECRET`).

#### `--google`

Uses Google Secret Manager. It expects:

1. `--google-project` - the GCP project name.

The app uses [`google-apis-rs`], thus supports all the [methods yup2-oauth supports]. You can
also specify credentials directly:

1. `--google-credentials-file` (or `GOOGLE_APPLICATION_CREDENTIALS`), or
2. `--google-credentials-json` (or `GOOGLE_APPLICATION_CREDENTIALS_JSON`).

The first one expects path to the credentials JSON file, the second one expects the **contents** of
the file.

#### `--vault`

Uses Hashicorp Vault.

It expects:

1. `--vault-address` - The address of the vault.

It does plain HTTPS requests, thus it expects:

1. `--vault-token` (or `VAULT_TOKEN`), and
2. `--vault-cacert` (or `VAULT_CACERT`).

### Secret storage modes

There are two possible modes of secret storage:

1. Environment stored as JSON in a single secret, or
2. Secrets being environment variables.

#### Env as JSON

This option is best if you want to store whole environment in a single place, or you want to store
multiple different environments in a single secret store and match it to the running program (e.g.
per project/tenant configuration). It expects a single key-value pair in the underlying storage,
where value is a JSON object with properties being non-complex values (so no arrays and no
objects).

Example JSON environment:

```json
{
    "Variable_A": false,
    "Variable_B": "Value",
    "Variable_C": 10
}
```

To get the environment as JSON, use the `--secret-name` option.

#### Prefixed mode

If you prefer storing a single environment variable as a single secret in the storage, you can use
prefixed mode. It finds all variables that start with a given prefix and interprets them as
environment variables. The prefix will be stripped from secret name before using it as environment
variable.

To get the environment as a list of prefixed secrets, use the `--secret-prefix` option.

##### A note on Azure KeyVault

Since AKV secrets cannot have `_` in the name, all `-` will be replaced with `_` (to follow the
convention used by ASP.NET Core).

##### A note on Hashicorp Vault

Since Vault stores a list of values for a single secret, `kvenv` adheres to that - it does not try
to force additional JSON encoding, it will get all pairs for a given secret directly.

When in prefixed mode, it gets all pairs for all the secrets that match the prefix and concatenate
them.

### Misc

#### Masking

`kvenv`, supports masking environment variables, i.e. hiding them from the destination process. This
can be achieved by using the `--mask` option, like so:

```sh
$ kvenv run-in ... --mask HOME --mask ANOTHER -- env
# There will be no `HOME` nor `ANOTHER` in the output
```

or

```sh
$ kvenv cache ... --mask HOME --mask ANOTHER
# There will be no `HOME` nor `ANOTHER` in the output cached file
```

Subsequent runs with the cached env file won't be able to see any of the mentioned variables.

## Features

* [x] Masking
* [x] JSON-based env keys
* [x] [ASP.NET Core-compatible](https://docs.microsoft.com/en-us/aspnet/core/security/key-vault-configuration?view=aspnetcore-5.0) key format
* [x] `run-with` with direct env credentials (a.k.a `run-in`)
* [x] Better documentation
* [x] Integration tests
* [x] GCP Secret Manager support
* [x] AWS Secrets Manager support
* [x] Hashicorp Vault support

## Help

The cmdline is [Clap](https://clap.rs/)-based, so you have convenient help out of the box:

```sh
$ kvenv help
A simple command-line utility that allows running arbitrary commands within a custom environment thatis loaded from Azure KeyVault, GCP Secret Manager, AWS Secrets Manager or Hashicorp Vault.

Usage: kvenv <COMMAND>

Commands:
  cache
          Caches the environment variables from KeyVault into local file
  run-with
          Runs the command with the specified argument using cached environment
  run-in
          Runs the command with the specified argument using freshly downloaded environment
  help
          Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
  -V, --version
          Print version
```

[`rusoto`]: https://github.com/rusoto/rusoto/
[AWS Credentials]: https://github.com/rusoto/rusoto/blob/master/AWS-CREDENTIALS.md
[`azure-sdk-for-rust`]: https://github.com/Azure/azure-sdk-for-rust
[Azure authentication methods]: https://github.com/Azure/azure-sdk-for-rust/blob/main/sdk/identity/examples/default_credentials.rs
[`google-apis-rs`]: https://github.com/Byron/google-apis-rs
[methods yup2-oauth supports]: https://docs.rs/yup-oauth2/latest/yup_oauth2/
