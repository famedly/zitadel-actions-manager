# Zitadel Actions Manager

[![rust workflow status][badge-rust-workflow-img]][badge-rust-workflow-url]
[![docker workflow status][badge-docker-workflow-img]][badge-docker-workflow-url]

[badge-rust-workflow-img]: https://github.com/famedly/rust-project-template/actions/workflows/rust.yml/badge.svg
[badge-rust-workflow-url]: https://github.com/famedly/rust-project-template/commits/main
[badge-docker-workflow-img]: https://github.com/famedly/rust-project-template/actions/workflows/docker.yml/badge.svg
[badge-docker-workflow-url]: https://github.com/famedly/rust-project-template/commits/main

A library and a CLI tool to sync/migrate [zitadel actions](https://zitadel.com/docs/apis/actions/introduction) defined in a declarative way.

## Data model

The actions are defined in `actions.yaml` file:
```yaml
action1:
  # string, optional, for the exact format dig the zitadel docs
  timeout: 'timeout'
  # bool, optional
  allowedToFail: false
  # string, optional, if not set a file action1.js will be sourced
  script: |
    function action1(ctx, api) {
      ...
    }

# action that needs to be deleted if it exists in zitadel
action2: deleted
```

The actual triggers are defined in `flows.yaml` file:
```yaml
FLOW_TYPE_EXTERNAL_AUTHENTICATION:
  TRIGGER_TYPE_PRE_CREATION: [action1]
```

Action names that are referenced in `flows.yaml` but not referenced in `actions.yaml` are presumed
to be defined in `actionName.js` files.

## Library usage

Depending on the scenario, you need to define the actions and triggers. You can do that statically
in code by just constructing `Actions<Loaded>` and `Flows` (with the help of `include!` macro) or have
actions defined in the files and loaded on start. For this scenario you'd need to read `actions.yaml`
and `flows.yaml`, deserialize them and call `load_actions` which will also load all action files
that are referenced but lack `script` field in `actions.yaml` file. Then call `sync` function.
See `run` function in [src/main.rs](src/main.rs) for reference.

You'd also need to implement `ZitadelHandle` for your zitadel client, or your can use
`simple_zitadel_client` included in this library (doesn't do oauth and token renewals).


## CLI tool usage

Install with
```sh
cargo install --features cli --path .
```

To perform sync/migration, run:
```sh
zitadel-actions-sync [OPTIONS]
```

Or run directly from source
```
cargo run --features cli -- [OPTIONS]
```

Options:
```
  -a, --actions <PATH>          File to read actions from [default: actions.yaml]
  -f, --flows <PATH>            File to read flows from [default: flows.yaml]
  -d, --dir <DIR>               Directory with actions [default: .]
  -u, --url <URL>               Zitadel Url [default: http://localhost:9310]
  -t, --token <TOKEN>           Zitadel access token [env: ZITADEL_JWT]
  -s, --service-account <PATH>  Zitadel service account file
      --aud <AUD>               Audience to add to zitadel JWT (used with `--service-account`)
  -o, --org-id <ORG_ID>         Organization for which perform the sync
  -l, --log-level <LOG_LEVEL>   Log level <off|trace|debug|warn|error> [env: LOG_LEVEL=] [default: info]
  -h, --help                    Print help
  -V, --version                 Print version
```

Example:
```sh
docker compose down -v
mkdir -p /tmp/zitadel-docker-test/
touch /tmp/zitadel-docker-test/service-account.json
docker compose up -d
docker compose wait ultimate_readiness_check
cargo run --features cli -- \
    -d example-actions \
    -s /tmp/zitadel-docker-test/service-account.json \
    -u http://localhost:9310 \
    --aud http://localhost:9310
```

## Testing
```sh
mkdir -p /tmp/zitadel-docker-test/
touch /tmp/zitadel-docker-test/service-account.json
docker compose down -v
docker compose up -d
docker compose wait ultimate_readiness_check
cargo test
```

Or all in one:
```sh
cargo nextest
```

## Pre-commit usage

1. If not installed, install with your package manager, or `pip install --user pre-commit`
2. Run `pre-commit autoupdate` to update the pre-commit config to use the newest template
3. Run `pre-commit install` to install the pre-commit hooks to your local environment

---

# Famedly

**This project is part of the source code of Famedly.**

We think that software for healthcare should be open source, so we publish most
parts of our source code at [github.com/famedly](https://github.com/famedly).

Please read [CONTRIBUTING.md](CONTRIBUTING.md) for details on our code of
conduct, and the process for submitting pull requests to us.

For licensing information of this project, have a look at the [LICENSE](LICENSE.md)
file within the repository.

If you compile the open source software that we make available to develop your
own mobile, desktop or embeddable application, and cause that application to
connect to our servers for any purposes, you have to agree to our Terms of
Service. In short, if you choose to connect to our servers, certain restrictions
apply as follows:

- You agree not to change the way the open source software connects and
  interacts with our servers
- You agree not to weaken any of the security features of the open source software
- You agree not to use the open source software to gather data
- You agree not to use our servers to store data for purposes other than
  the intended and original functionality of the Software
- You acknowledge that you are solely responsible for any and all updates to
  your software

No license is granted to the Famedly trademark and its associated logos, all of
which will continue to be owned exclusively by Famedly GmbH. Any use of the
Famedly trademark and/or its associated logos is expressly prohibited without
the express prior written consent of Famedly GmbH.

For more
information take a look at [Famedly.com](https://famedly.com) or contact
us by [info@famedly.com](mailto:info@famedly.com?subject=[GitLab]%20More%20Information%20)
