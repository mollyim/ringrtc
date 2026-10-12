use std::time::SystemTime;

use base64::{Engine, prelude::BASE64_STANDARD};
use hex::{FromHex, ToHex};
use hmac::{Hmac, KeyInit, Mac};
use itertools::Itertools;
use sha2::{Digest, Sha256};

use crate::common::{ClientProfile, Group, GroupMember, GroupMetadata};

type HmacSha256 = Hmac<Sha256>;
const GV2_AUTH_MATCH_LIMIT: usize = 10;

#[derive(Debug, Clone)]
pub struct DynamicClientProfileFactory {
    group_auth_key: [u8; 32],
}

impl DynamicClientProfileFactory {
    #[allow(dead_code)]
    pub fn new() -> Self {
        let key = <[u8; 32]>::from_hex(
            "deaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddead",
        )
        .unwrap();
        Self::new_with_key(key)
    }

    pub fn new_with_key(group_auth_key: [u8; 32]) -> Self {
        Self { group_auth_key }
    }

    pub fn client_profiles_for_group<T: AsRef<str>>(
        &self,
        group_name: &str,
        names: &[T],
        sys_now: SystemTime,
    ) -> Vec<ClientProfile> {
        let client_ids = names
            .iter()
            .map(|_| {
                let user_id_hex = gen_uuid();
                let member_id_hex = format!("{}{}", gen_uuid(), gen_uuid());
                (user_id_hex, member_id_hex)
            })
            .collect_vec();
        let members = client_ids
            .iter()
            .map(|(user_id_hex, member_id_hex)| {
                let user_id_base64 = BASE64_STANDARD.encode(hex::decode(user_id_hex).unwrap());
                let member_id_base64 = BASE64_STANDARD.encode(hex::decode(member_id_hex).unwrap());

                GroupMember {
                    user_id: user_id_base64,
                    member_id: member_id_base64,
                }
            })
            .collect();

        let group_id_hex = gen_uuid();
        let group_id_base64 = BASE64_STANDARD.encode(hex::decode(&group_id_hex).unwrap());
        let group_metadata = {
            GroupMetadata {
                name: group_name.to_owned(),
                id_base64: group_id_base64,
                members,
            }
        };

        client_ids
            .into_iter()
            .map(|(user_id_hex, member_id_hex)| {
                let timestamp = sys_now
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let membership_proof = BASE64_STANDARD.encode(generate_signed_v2_password(
                    &member_id_hex,
                    &group_id_hex,
                    timestamp,
                    ALL_PERMISSIONS,
                    &self.group_auth_key,
                ));

                let groups = vec![Group {
                    metadata: group_metadata.clone(),
                    membership_proof,
                }];

                ClientProfile {
                    user_id: user_id_hex,
                    device_id: "1".to_string(),
                    groups,
                }
            })
            .collect()
    }
}

fn gen_uuid() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "")
}

const ALL_PERMISSIONS: &str = "1";

fn generate_signed_v2_password(
    user_id_hex: &str,
    group_id_hex: &str,
    timestamp: u64,
    permission: &str,
    key: &[u8; 32],
) -> String {
    let opaque_user_id = sha256_as_hexstring(&hex::decode(user_id_hex).unwrap());
    // Format the credentials string.
    let credentials = format!(
        "2:{}:{}:{}:{}",
        opaque_user_id, group_id_hex, timestamp, permission
    );

    // Get the MAC for the credentials.
    let mut hmac = HmacSha256::new_from_slice(key).unwrap();
    hmac.update(credentials.as_bytes());
    let mac = hmac.finalize().into_bytes();
    let mac = &mac[..GV2_AUTH_MATCH_LIMIT];

    // Append the MAC to the credentials.
    format!("{}:{}", credentials, mac.encode_hex::<String>())
}

fn sha256_as_hexstring(data: &[u8]) -> String {
    Sha256::digest(data).encode_hex()
}
