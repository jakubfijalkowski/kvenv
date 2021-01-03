kvenv
=====

`kvenv` is a simple command-line utility written in Rust that allows running arbitrary command with
a custom environment that is loaded from Azure KeyVault.

Note: the tool is early stage but should be usable already

## Example usage
```sh
$ kvenv cache \
    --tenant-id e96760c2-66ca-430e-9ecc-4556eeee59d7 \ # Tenant ID
    --client-id 54bdd66e-b650-4ad8-8a37-2c135cd5f5ff \ # Application (Client) ID
    --client-secret appsecret \ # The 'password' or 'secret' of the application
    --keyvault-name example-keyvault \ # The name of the keyvault where the secret is stored (and app has access)
    --secret-name test
/tmp/kvenv-xxxxx.json
$ kvenv run-with --env-file /tmp/kvenv-xxxxx.json -- env
...
$ kvenv cleanup /tmp/kvenv-xxxxx.json
```

The main usage is in CI/CD pipelines - if your tool of choice does not support convenient,
per-project secrets (or the functionality does not support multitenancy) management, you might store
the secrets in KV and then subsequently load it using the `kvenv` tools, without exposing raw
credentials to the processes.

Explanatory post is coming soon.

## Features

- [x] Masking
- [x] JSON-based env keys
- [ ] [ASP.NET Core-compatible](https://docs.microsoft.com/en-us/aspnet/core/security/key-vault-configuration?view=aspnetcore-5.0) key format
- [x] `run-with` with direct env credentials (a.k.a `run-in`)
- [ ] Better documentation
- [x] Integration tests

## Environment format

The environemnt needs to be stored as a single secret inside single Azure KeyVault. It needs to be
encoded as a simple JSON-map, with string-y values (i.e. strings, `null`, number and boolean).
`kvenv` will then load the secret, deserialize JSON and store it in the local file.

## Masking

`kvenv`, apart from loading the env from Azure KV, supports masking environment variables, i.e.
hiding them from the destination process. This can be achieved by using the `--mask` (or `-m`)
switch in the `cache` command, like so:

```sh
$ kvenv cache ... --mask HOME --mask ANOTHER
```

Subsequent runs with the cached env file won't be able to see any of the mentioned variables.

## Help

The cmdline is [Clap](https://clap.rs/)-based, so you have convenient help out of the box:
```sh
$ kvenv help
```
