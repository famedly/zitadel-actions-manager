// SPDX-FileCopyrightText: 2025 Famedly GmbH (info@famedly.com)
//
// SPDX-License-Identifier: Apache-2.0

//! Actions v2 sync.
use std::{
    collections::BTreeMap as Map,
    path::{Path, PathBuf},
};

use as_variant::as_variant;
use famedly_rust_utils::GenericCombinators;
use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;
use tracing::info;

use crate::{
    InvalidPublicKeyConfig, ReadFile, ReadYamlFileError, from_yaml_file, instrument,
    zitadel::{
        AddPublicKey, CreateTarget, Execution, FoundTarget, PayloadType, TargetType, UpdateTarget,
        ZitadelHandleV2, decode_public_key_pem, expiration_dates_eq, normalize_pem,
        parse_expiration_date,
    },
};

#[doc(hidden)]
pub const DEFAULT_TARGETS_FILE: &str = "targets.yaml";
#[doc(hidden)]
pub const DEFAULT_EXECUTIONS_FILE: &str = "executions.yaml";

/// Targets definitions (`targets.yaml`)
///
/// ```yaml
/// target1:
///   restAsync: {}
///   endpoint: http://example.com/call_me
///   timeout: 5s
///   payloadType: PAYLOAD_TYPE_JSON
///
/// encrypted_target:
///   restCall:
///     interruptOnError: true
///   endpoint: http://example.com/jwe
///   timeout: 5s
///   payloadType: PAYLOAD_TYPE_JWE
///   publicKeyFile: keys/target.pem
///   # or inline:
///   # publicKey: |
///   #   -----BEGIN PUBLIC KEY-----
///   #   ...
///   # publicKeyExpiration: "2027-01-01T00:00:00Z"
///
/// # delete target2 target if it exists
/// target2: null
/// ```
pub type Targets = Map<String, Option<Target>>;

/// Execution definitions (`executions.yaml`)
///
/// ```yaml
/// - condition: {event: {event: user.human.added}}
///   targets: [target1] # targets by their names defined in targets.yaml
///
/// - condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
///   targets: [target1]
/// ```
pub type Executions = Vec<Execution>;

/// Public key for [`PayloadType::Jwe`] targets: inline PEM or path to a PEM
/// file (resolved into [`PublicKey::Inlined`] during [`load`]).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PublicKey {
    /// Inline PEM public key.
    #[serde(rename_all = "camelCase")]
    Inlined { public_key: String },
    /// Path to a PEM public key file.
    #[serde(rename_all = "camelCase")]
    File { public_key_file: PathBuf },
}

impl PublicKey {
    /// PEM content when this is an inlined key.
    #[must_use]
    pub fn as_pem(&self) -> Option<&str> {
        match self {
            Self::Inlined { public_key } => Some(public_key.as_str()),
            Self::File { .. } => None,
        }
    }
}

/// Target definition. Reflects HTTP API types exposed by Zitadel.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    #[serde(flatten)]
    pub target_type: TargetType,
    pub timeout: String,
    pub endpoint: url::Url,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
    /// Public key for [`PayloadType::Jwe`] (`publicKey` or `publicKeyFile`).
    #[serde(default, flatten, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<PublicKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key_expiration: Option<String>,
}

// TODO: it's very likely that zitadel returns some optional fields filled with
// default values, we should account for it, similar to zitadel::action_is_same
impl PartialEq<FoundTarget> for Target {
    fn eq(&self, other: &FoundTarget) -> bool {
        self.target_type == other.target_type
            && self.timeout == other.timeout
            && self.endpoint.as_str() == other.endpoint
            && PayloadType::eq_optional(self.effective_payload_type(), other.payload_type)
    }
}

impl Target {
    /// Payload type sent to Zitadel: explicit value, or JWE when a public key
    /// is configured, otherwise omitted (Zitadel default JSON).
    #[must_use]
    pub fn effective_payload_type(&self) -> Option<PayloadType> {
        match self.payload_type {
            Some(pt) => Some(pt),
            None if self.public_key.is_some() => Some(PayloadType::Jwe),
            None => None,
        }
    }

    /// Concrete desired payload type for comparisons and updates.
    /// Implicit omit / UNSPECIFIED maps to JSON (Zitadel's default).
    #[must_use]
    pub fn desired_payload_type(&self) -> PayloadType {
        self.effective_payload_type().unwrap_or(PayloadType::Json).normalized()
    }

    /// Build an update request. On update, omitted `payloadType` means "leave
    /// unchanged", so the implicit default must be sent as explicit JSON when
    /// the remote target is not already default.
    #[must_use]
    pub fn to_update_target(&self, existing: &FoundTarget) -> UpdateTarget {
        let desired = self.desired_payload_type();
        let payload_type = if PayloadType::eq_optional(Some(desired), existing.payload_type) {
            None
        } else {
            Some(desired)
        };
        UpdateTarget {
            target_type: Some(self.target_type.clone()),
            timeout: Some(self.timeout.clone()),
            endpoint: Some(self.endpoint.to_string()),
            expiration_signing_key: None,
            payload_type,
        }
    }

    #[must_use]
    pub fn to_create_target(&self, name: String) -> CreateTarget {
        CreateTarget {
            name,
            target_type: self.target_type.clone(),
            timeout: self.timeout.clone(),
            endpoint: self.endpoint.to_string(),
            payload_type: self.effective_payload_type(),
        }
    }

    fn resolve_public_key(&mut self, dir: &Path) -> Result<(), ReadYamlFileError> {
        match self.public_key.take() {
            Some(PublicKey::File { public_key_file }) => {
                let path = dir.join(public_key_file);
                let pem = std::fs::read_to_string(&path).context(ReadFile { path })?;
                self.public_key = Some(PublicKey::Inlined { public_key: pem });
            }
            other => self.public_key = other,
        }

        let effective = self.effective_payload_type();
        if effective == Some(PayloadType::Jwe) && self.public_key.is_none() {
            return InvalidPublicKeyConfig {
                message: String::from("payloadType JWE requires publicKey or publicKeyFile"),
            }
            .fail();
        }
        (self.public_key.is_none()
            || !matches!(self.payload_type, Some(PayloadType::Json | PayloadType::Jwt)))
        .ok_or_else(|| {
            InvalidPublicKeyConfig {
                message: String::from("publicKey is only valid with payloadType JWE"),
            }
            .build()
        })?;
        if let Some(expiration) = &self.public_key_expiration {
            parse_expiration_date(expiration)
                .map_err(|err| InvalidPublicKeyConfig { message: err.to_string() }.build())?;
        }
        Ok(())
    }
}

/// Syncs v2 provided loaded targets and executions with running Zitadel
/// instance. Targets marked as `None` will be deleted.
#[instrument(skip_all)]
pub async fn sync<Z: ZitadelHandleV2>(
    zitadel: &Z,
    targets: Targets,
    executions: Executions,
) -> Result<(), Z::Err> {
    // 1. Fetch existing Targets from zitadel (by names referenced in `targets`)
    let mut pre_existing_targets: Map<String, _> = Map::new();
    info!("Fetching all locally defined actions by their names");
    for name in targets.keys() {
        if let Some(action) = zitadel.search_target_by_name(name).await? {
            pre_existing_targets.insert(name.clone(), action);
        }
    }
    info!(
        "Fetched {} targets out of {} defined locally",
        pre_existing_targets.len(),
        targets.len()
    );

    let names_to_delete = targets
        .iter()
        .filter_map(|(name, target)| as_variant!(target, None => name.into()))
        .collect::<Vec<String>>();
    let targets_to_update = targets
        .into_iter()
        .filter_map(|(name, target)| as_variant!(target, Some(target) => (name, target)));

    // 2. Create and update Targets
    let mut existing_targets = Map::new();
    for (name, target) in targets_to_update {
        let target_id = if let Some(their_target) = pre_existing_targets.remove(&name) {
            if target == their_target {
                info!(%name, target_id = %their_target.id, "Target is unchanged, skipping");
            } else {
                info!(%name, target_id = %their_target.id, "Updating target");
                zitadel
                    .update_target(&their_target.id, target.to_update_target(&their_target))
                    .await?;
            }
            their_target.id
        } else {
            info!(%name, "New target detected, creating");

            let target_id = zitadel.create_target(target.to_create_target(name.clone())).await?.id;
            info!(%name, %target_id, "Created target");
            target_id
        };

        ensure_public_key(zitadel, &name, &target_id, &target).await?;
        existing_targets.insert(name, target_id);
    }

    // 3. Set Executions
    let mut existing_executions = zitadel.list_executions().await?;
    existing_executions.iter_mut().for_each(|execution| execution.targets.sort());
    for mut execution in executions.into_iter() {
        let target_ids = execution
            .targets
            .iter()
            .filter_map(|name| existing_targets.get(name).cloned())
            .collect::<Vec<_>>()
            .mutate(|ids| ids.sort()); // TODO: figure out if targets order in a
        // execution matters
        let condition = &execution.condition;

        if let Some(existing_execution) =
            existing_executions.iter().find(|execution| &execution.condition == condition)
            && existing_execution.targets == target_ids
        {
            info!(?condition, ?target_ids, "Triggers are unchanged, skipping");
            continue;
        }

        info!(?condition, ?target_ids, "Setting targets execution");
        execution.targets = target_ids;
        zitadel.set_execution(execution).await?;
    }

    // 4. Delete Targets that are marked as null
    for (id, name) in names_to_delete
        .into_iter()
        .filter_map(|name| pre_existing_targets.remove(&name).map(|target| (target.id, name)))
    {
        info!(%id, %name, "Deleting action");
        zitadel.delete_target(&id).await?;
    }

    info!("Sync successful");

    Ok(())
}

#[instrument(skip(zitadel, target))]
async fn ensure_public_key<Z: ZitadelHandleV2>(
    zitadel: &Z,
    name: &str,
    target_id: &str,
    target: &Target,
) -> Result<(), Z::Err> {
    let Some(pem) = target.public_key.as_ref().and_then(PublicKey::as_pem) else {
        return Ok(());
    };
    let wanted = normalize_pem(pem);
    let wanted_expiration = target.public_key_expiration.as_deref();

    let keys = zitadel.list_public_keys(target_id).await?;
    let matching = keys
        .into_iter()
        .find_map(|key| {
            let remote_pem = key.public_key.as_deref().and_then(|encoded| {
                decode_public_key_pem(encoded).ok().map(|pem| normalize_pem(&pem))
            });
            match (
                remote_pem.as_ref() == Some(&wanted),
                expiration_dates_eq(key.expiration_date.as_deref(), wanted_expiration),
            ) {
                (_, Err(err)) => Some(Err(err)),
                (true, Ok(true)) => Some(Ok(key)),
                _ => None,
            }
        })
        .transpose()?;

    let key_id = if let Some(key) = matching {
        if key.active {
            info!(%name, %target_id, key_id = %key.key_id, "Public key already active, skipping");
            return Ok(());
        }
        info!(%name, %target_id, key_id = %key.key_id, "Activating existing public key");
        key.key_id
    } else {
        info!(%name, %target_id, "Uploading public key");
        let added = zitadel
            .add_public_key(
                target_id,
                AddPublicKey {
                    public_key: pem.to_owned(),
                    expiration_date: target.public_key_expiration.clone(),
                },
            )
            .await?;
        info!(%name, %target_id, key_id = %added.key_id, "Activating uploaded public key");
        added.key_id
    };

    zitadel.activate_public_key(target_id, &key_id).await?;
    Ok(())
}

/// Loads v2 targets and executions.
///
/// - `dir` is a directory path to where source targets and executions.
/// - `targets` is an optional path relative to `dir`, `targets.yaml` by default.
/// - `executions` is an optional path relative to `dir`, `executions.yaml` by default.
///
/// Resolves `publicKeyFile` into inline `publicKey` PEM content.
#[instrument]
pub fn load(
    dir: &Path,
    targets: Option<&Path>,
    executions: Option<&Path>,
) -> Result<(Targets, Executions), ReadYamlFileError> {
    let targets_fname = dir.join(targets.unwrap_or(Path::new(DEFAULT_TARGETS_FILE)));
    let executions_fname = dir.join(executions.unwrap_or(Path::new(DEFAULT_EXECUTIONS_FILE)));
    let mut targets: Targets = from_yaml_file(&targets_fname)?;
    let executions = from_yaml_file(&executions_fname)?;
    for target in targets.values_mut().flatten() {
        target.resolve_public_key(dir)?;
    }
    Ok((targets, executions))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::zitadel::{FoundTarget, TargetType};

    const SAMPLE_PEM: &str = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAoAL7geWB02K3b90hAV9M\nQIatE+L0Hj6b1uuFf4iOGkgVeqnxfnwqtZsEU3xxUxGhMZ2Qjr7xrODMBxnI+tos\nDuoeGVmCqYg+szRtca4XhxakoFMZV9JnZDH862qvqKICHhZj796OUwCM00MfozCE\nI5/kWKGWoP39koVoaB8UKDrrSUvB8yH1lbB8F4tOBzED9K4BRle4u1WW/Dl74Y4a\ngysWRYrxVxbQN3AFJq5V+93EpEFhIxZT4PIuL+v2yLxHOB/x9N1FKxxq4AxDROMv\nOOM2mE9Dzif/JR/30BFs6aZL0ciASQva4X9Fskdlal3qk5Kn2pNNXYXk0FQPS50t\nTwIDAQAB\n-----END PUBLIC KEY-----\n";

    fn write_targets_and_executions(dir: &Path, targets: &str) {
        fs::write(dir.join("targets.yaml"), targets).unwrap();
        fs::write(
            dir.join("executions.yaml"),
            "- condition: {event: {event: user.human.added}}\n  targets: []\n",
        )
        .unwrap();
    }

    fn sample_found_target(payload_type: Option<PayloadType>) -> FoundTarget {
        FoundTarget {
            id: "id".into(),
            name: "name".into(),
            target_type: TargetType::restAsync {},
            timeout: "5s".into(),
            endpoint: "http://example.com/".into(),
            signing_key: "key".into(),
            payload_type,
        }
    }

    #[test]
    fn load_rejects_jwe_without_public_key() {
        let dir = tempfile::tempdir().unwrap();
        write_targets_and_executions(
            dir.path(),
            r#"
t:
  restAsync: {}
  endpoint: http://example.com
  timeout: 5s
  payloadType: PAYLOAD_TYPE_JWE
"#,
        );
        let err = load(dir.path(), None, None).unwrap_err();
        assert!(matches!(err, ReadYamlFileError::InvalidPublicKeyConfig { .. }));
    }

    #[test]
    fn load_rejects_invalid_public_key_expiration() {
        let dir = tempfile::tempdir().unwrap();
        let targets = format!(
            "t:\n  restAsync: {{}}\n  endpoint: http://example.com\n  timeout: 5s\n  payloadType: PAYLOAD_TYPE_JWE\n  publicKeyExpiration: not-a-date\n  publicKey: |\n{}",
            SAMPLE_PEM.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n")
        );
        write_targets_and_executions(dir.path(), &targets);
        let err = load(dir.path(), None, None).unwrap_err();
        assert!(
            matches!(err, ReadYamlFileError::InvalidPublicKeyConfig { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn load_rejects_public_key_with_json_payload() {
        let dir = tempfile::tempdir().unwrap();
        let targets = format!(
            "t:\n  restAsync: {{}}\n  endpoint: http://example.com\n  timeout: 5s\n  payloadType: PAYLOAD_TYPE_JSON\n  publicKey: |\n{}",
            SAMPLE_PEM.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n")
        );
        write_targets_and_executions(dir.path(), &targets);
        let err = load(dir.path(), None, None).unwrap_err();
        assert!(
            matches!(err, ReadYamlFileError::InvalidPublicKeyConfig { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn load_resolves_public_key_file() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let mut key_file = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        key_file.write_all(SAMPLE_PEM.as_bytes()).unwrap();
        let key_file_name = key_file.path().file_name().unwrap().to_str().unwrap();
        write_targets_and_executions(
            dir.path(),
            &format!(
                r#"
t:
  restAsync: {{}}
  endpoint: http://example.com
  timeout: 5s
  payloadType: PAYLOAD_TYPE_JWE
  publicKeyFile: {key_file_name}
"#
            ),
        );
        let (targets, _) = load(dir.path(), None, None).unwrap();
        let target = targets.get("t").unwrap().as_ref().unwrap();
        assert_eq!(target.public_key.as_ref().and_then(PublicKey::as_pem), Some(SAMPLE_PEM));
        assert_eq!(target.effective_payload_type(), Some(PayloadType::Jwe));
    }

    #[test]
    fn target_partial_eq_compares_payload_type_with_defaults() {
        let target = Target {
            target_type: TargetType::restAsync {},
            timeout: "5s".into(),
            endpoint: "http://example.com/".parse().unwrap(),
            payload_type: None,
            public_key: None,
            public_key_expiration: None,
        };
        assert_eq!(target, sample_found_target(None));
        assert_eq!(target, sample_found_target(Some(PayloadType::Json)));
        assert_eq!(target, sample_found_target(Some(PayloadType::Unspecified)));
        assert_ne!(target, sample_found_target(Some(PayloadType::Jwt)));
    }

    fn sample_target(payload_type: Option<PayloadType>) -> Target {
        Target {
            target_type: TargetType::restAsync {},
            timeout: "5s".into(),
            endpoint: "http://example.com/".parse().unwrap(),
            payload_type,
            public_key: None,
            public_key_expiration: None,
        }
    }

    #[test]
    fn to_update_target_sends_json_when_resetting_from_jwt() {
        let update =
            sample_target(None).to_update_target(&sample_found_target(Some(PayloadType::Jwt)));
        assert_eq!(update.payload_type, Some(PayloadType::Json));
    }

    #[test]
    fn to_update_target_omits_payload_type_when_already_default() {
        let update =
            sample_target(None).to_update_target(&sample_found_target(Some(PayloadType::Json)));
        assert_eq!(update.payload_type, None);

        let update = sample_target(None).to_update_target(&sample_found_target(None));
        assert_eq!(update.payload_type, None);
    }

    #[test]
    fn to_update_target_sends_jwt_when_changing_from_json() {
        let update = sample_target(Some(PayloadType::Jwt))
            .to_update_target(&sample_found_target(Some(PayloadType::Json)));
        assert_eq!(update.payload_type, Some(PayloadType::Jwt));
    }
}
