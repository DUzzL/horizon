//! The management protocol's list edits, which `horizon-server` forwards on the control endpoint as
//! `management <connection> {"method", "entries"}` (its management_methods.rs): vanilla 26.3's
//! `AllowlistService` and `BanlistService` on the running server's lists. Players are found as
//! `MinecraftPlayerListService.getUser` finds them, each change is logged as `JsonRpcLogger` logs it, and the
//! players a change shuts out are disconnected. The supervisor answers with the lists, from their files.
use super::{ServerState, log};
use crate::game::Game;
use horizon_client::{
    server::{
        profiles,
        user_lists::{BanEntry, NameAndId, WhiteEntry},
    },
    text::Text,
};
use horizon_protocol::text::translatable;
use serde_json::{Value, json};

/// A `UserBan`: the player, the reason, the source, the end.
type Ban = BanEntry<NameAndId>;

/// `NameAndId`'s record `toString`, as vanilla's log lines show it.
fn shown(user: &NameAndId) -> String {
    format!(
        "NameAndId[id={}, name={}]",
        horizon_protocol::crypto::uuid_string(user.id),
        user.name
    )
}

impl Game<'_> {
    pub(super) fn management(&mut self, state: &mut ServerState, request: &str) -> Value {
        let (connection, body) = request.split_once(' ').unwrap_or((request, "{}"));
        let rpc =
            |message: String| log("INFO", &format!("RPC Connection #{connection}: {message}"));
        let Ok(body) = serde_json::from_str::<Value>(body) else {
            return json!({ "error": "a management request is a connection number and a JSON object" });
        };
        let entries = body["entries"].as_array().cloned().unwrap_or_default();
        let lists = state.lists.clone();
        match body["method"].as_str().unwrap_or_default() {
            "allowlist/add" => {
                for user in entries.iter().filter_map(|e| self.management_user(e)) {
                    rpc(format!("Add player '{}' to allowlist", shown(&user)));
                    lists.lock().unwrap().white.add(WhiteEntry { user });
                }
            }
            "allowlist/remove" => {
                for user in entries.iter().filter_map(|e| self.management_user(e)) {
                    rpc(format!("Remove player '{}' from allowlist", shown(&user)));
                    lists.lock().unwrap().white.remove(&user);
                }
                rpc("Kick unlisted players".into());
                self.kick_unlisted_players(&lists);
            }
            "allowlist/set" => {
                let wanted: Vec<NameAndId> = entries
                    .iter()
                    .filter_map(|e| self.management_user(e))
                    .collect();
                let current: Vec<NameAndId> = lists
                    .lock()
                    .unwrap()
                    .white
                    .entries()
                    .iter()
                    .map(|e| e.user.clone())
                    .collect();
                for user in current.iter().filter(|u| !wanted.contains(u)) {
                    rpc(format!("Remove player '{}' from allowlist", shown(user)));
                    lists.lock().unwrap().white.remove(user);
                }
                for user in wanted.iter().filter(|u| !current.contains(u)) {
                    rpc(format!("Add player '{}' to allowlist", shown(user)));
                    lists
                        .lock()
                        .unwrap()
                        .white
                        .add(WhiteEntry { user: user.clone() });
                }
                rpc("Kick unlisted players".into());
                self.kick_unlisted_players(&lists);
            }
            "allowlist/clear" => {
                rpc("Clear allowlist".into());
                lists.lock().unwrap().white.clear();
            }
            "bans/add" => {
                let bans: Vec<Ban> = entries
                    .iter()
                    .filter_map(|e| self.management_ban(e))
                    .collect();
                for ban in bans {
                    self.management_add_ban(&rpc, state, ban);
                }
            }
            "bans/remove" => {
                for user in entries.iter().filter_map(|e| self.management_user(e)) {
                    rpc(format!("Remove player '{}' from banlist", shown(&user)));
                    lists.lock().unwrap().bans.remove(&user);
                }
            }
            "bans/set" => {
                let wanted: Vec<Ban> = entries
                    .iter()
                    .filter_map(|e| self.management_ban(e))
                    .collect();
                let current: Vec<Ban> = lists
                    .lock()
                    .unwrap()
                    .bans
                    .entries()
                    .into_iter()
                    .cloned()
                    .collect();
                for ban in current.iter().filter(|b| !wanted.contains(b)) {
                    rpc(format!("Remove player '{}' from banlist", shown(&ban.user)));
                    lists.lock().unwrap().bans.remove(&ban.user);
                }
                for ban in wanted.into_iter().filter(|b| !current.contains(b)) {
                    self.management_add_ban(&rpc, state, ban);
                }
            }
            // MinecraftBanListServiceImpl.clearUserBans logs nothing.
            "bans/clear" => lists.lock().unwrap().bans.clear(),
            other => return json!({ "error": format!("unknown management method {other:?}") }),
        }
        json!({ "ok": true })
    }

    /// `addUserBan`, then the banned player, if online, is sent away.
    fn management_add_ban(&mut self, rpc: &impl Fn(String), state: &ServerState, ban: Ban) {
        let reason = ban.reason.clone().unwrap_or_else(|| {
            Text::translate("multiplayer.disconnect.banned.reason.default", &[]).plain()
        });
        rpc(format!(
            "Add player '{}' to banlist. Reason: '{reason}'",
            ban.user.name
        ));
        let id = ban.user.id;
        state.lists.lock().unwrap().bans.add(ban);
        self.kick_uuid(id, translatable("multiplayer.disconnect.banned", &[]));
    }

    /// `UserBanDto.toUserBan`: the player found, the source "Management server" unless one is given; the ban
    /// starts now.
    fn management_ban(&self, entry: &Value) -> Option<Ban> {
        let user = self.management_user(&entry["player"])?;
        let source = entry["source"].as_str().unwrap_or("Management server");
        let reason = entry["reason"].as_str().map(str::to_owned);
        Some(BanEntry::new(
            user,
            Some(source.to_owned()),
            entry["expires"].as_i64(),
            reason,
        ))
    }

    /// `MinecraftPlayerListService.getUser`: by id, the user cache, else the session service; by name, the
    /// user cache, else the profile service (an offline server's offline player).
    fn management_user(&self, player: &Value) -> Option<NameAndId> {
        if let Some(id) = player["id"]
            .as_str()
            .and_then(|s| u128::from_str_radix(s, 16).ok())
        {
            let cached = super::USER_CACHE
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|c| c.get_by_id(id));
            return cached.or_else(|| profiles::find_by_id(&profiles::discovery_url(), id));
        }
        self.resolve_name(player["name"].as_str()?)
    }
}
