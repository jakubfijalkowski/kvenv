kvenv
=====

`kvenv` is a simple command-line utility written in Rust that allows running arbitrary command with
a custom environment that is loaded from Azure KeyVault, GCP Secret Manager, AWS Secrets Manager or
Hashicorp Vault.

Note: the tool is early stage but should be usable already

## Example usage
```sh
$ kvenv cache \
    --azure \
    --azure-tenant-id e96760c2-66ca-430e-9ecc-4556eeee59d7 \ # Tenant ID
    --azure-client-id 54bdd66e-b650-4ad8-8a37-2c135cd5f5ff \ # Application (Client) ID
    --azure-client-secret appsecret \ # The 'password' or 'secret' of the application
    --azure-keyvault-name example-keyvault \ # The name of the keyvault where the secret is stored (and app has access)
    --secret-name test
/tmp/kvenv-xxxxx.json
$ kvenv run-with --env-file /tmp/kvenv-xxxxx.json -- env
...
$ rm /tmp/kvenv-xxxxx.json
```

or

```sh
$ kvenv run-in \
    --azure
    --azure-tenant-id e96760c2-66ca-430e-9ecc-4556eeee59d7 \ # Tenant ID
    --azure-client-id 54bdd66e-b650-4ad8-8a37-2c135cd5f5ff \ # Application (Client) ID
    --azure-client-secret appsecret \ # The 'password' or 'secret' of the application
    --azure-keyvault-name example-keyvault \ # The name of the keyvault where the secret is stored (and app has access)
    --secret-name test \
    -- env
...
```

The main usage is in CI/CD pipelines - if your tool of choice does not support convenient,
per-project secrets (or the functionality does not support multitenancy) management, you might store
the secrets in KV and then subsequently load it using the `kvenv` tools, without exposing raw
credentials to the processes.

Explanatory post is coming soon.

## Features

- [x] Masking
- [x] JSON-based env keys
- [x] [ASP.NET Core-compatible](https://docs.microsoft.com/en-us/aspnet/core/security/key-vault-configuration?view=aspnetcore-5.0) key format
- [x] `run-with` with direct env credentials (a.k.a `run-in`)
- [ ] Better documentation
- [x] Integration tests
- [x] GCP Secret Manager support
- [x] AWS Secrets Manager support
- [x] Hashicorp Vault support

## Environment format

### Azure & Google & AWS

When loading environment from a single key (i.e. specifying `secret-name` instead of
`secret-prefix`), the secret will be interpreted as a JSON document that contains all the variables.
It needs to be encoded as a simple JSON-map, with string-y values (i.e. strings, `null`, number and
boolean). `kvenv` will then load the secret, deserialize JSON and pass it further.

When using prefixed mode, all secrets that are found will be used as environment variables, meaning
each secret will be a single variable, with the secret contents being variable value.

### Hashicorp Vault

Hashicorp Vault integration supports only Secrets engine v2 (it is tested agains it). Since Vault
treats each secret as a key-value map, it does not need to be JSON-encoded. `kvenv` will get all
pairs and interpret them as environment variables.

When in prefixed mode, all secrets that are found will be downloaded and the values will be
concatenated.

## Masking

`kvenv`, supports masking environment variables, i.e. hiding them from the destination process. This
can be achieved by using the `--mask` (or `-m`) switch in the `cache` command, like so:

```sh
$ kvenv cache ... --mask HOME --mask ANOTHER
```

Subsequent runs with the cached env file won't be able to see any of the mentioned variables.

## Help

The cmdline is [Clap](https://clap.rs/)-based, so you have convenient help out of the box:
```sh
$ kvenv help
```
