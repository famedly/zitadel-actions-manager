# Zitadel Actions Manager

[![rust workflow status][badge-rust-workflow-img]][badge-rust-workflow-url]
[![docker workflow status][badge-docker-workflow-img]][badge-docker-workflow-url]

[badge-rust-workflow-img]: https://github.com/famedly/rust-project-template/actions/workflows/rust.yml/badge.svg
[badge-rust-workflow-url]: https://github.com/famedly/rust-project-template/commits/main
[badge-docker-workflow-img]: https://github.com/famedly/rust-project-template/actions/workflows/docker.yml/badge.svg
[badge-docker-workflow-url]: https://github.com/famedly/rust-project-template/commits/main

A library and a CLI tool to sync/migrate [zitadel actions](https://zitadel.com/docs/apis/actions/introduction).

## Installation
```sh
cargo install --path .
```

## CLI tool usage

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

The action names that are referenced in `flows.yaml` but not referenced in `actions.yaml` are going to be sourced from `actionName.js` files.

To perform sync/migration, run:
```sh
zitadel-actions-sync -t $ZITADEL_ACCESS_TOKEN [OPTIONS]
```

Options:
```
  -a, --actions <ACTIONS>  File to read actions from [default: actions.yaml]
  -f, --flows <FLOWS>      File to read flows from [default: flows.yaml]
  -d, --dir <DIR>          Directory with actions [default: .]
  -u, --url <URL>          Zitadel Url [default: localhost:9310]
  -t, --token <TOKEN>      Zitadel access token
  -o, --org-id <ORG_ID>    Organization for which perform the sync
  -h, --help               Print help
  -V, --version            Print version
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
