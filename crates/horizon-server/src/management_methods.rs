//! The methods of vanilla's management protocol (`management.rs`), as vanilla 26.3's `methods/*Service` answer
//! them: the players, the server's state, and the lists. What changes the game goes through its console, so
//! it logs and saves as an operator's command would.

use crate::{
    api::{Phase, Shared},
    engine,
    management::{Failure, INTERNAL_ERROR, invalid_params},
};
use serde_json::{Value, json};
use std::{path::Path, sync::atomic::Ordering};

/// A method's result (`short`: its name without `minecraft:`).
pub fn call(shared: &Shared, short: &str, argument: &Value) -> Result<Value, Failure> {
    let (game, lists) = (Game { shared }, Lists(&shared.dir));
    match short {
        "players" => Ok(Value::Array(game.online()?)),
        "players/kick" => game.kick(argument),
        "server/status" => Ok(game.status()),
        "server/save" => {
            let flush = argument
                .as_bool()
                .ok_or_else(|| invalid_params("Not a boolean"))?;
            game.console(if flush { "save-all flush" } else { "save-all" })?;
            Ok(json!(true))
        }
        "server/stop" => {
            shared.stop.store(true, Ordering::SeqCst);
            Ok(json!(true))
        }
        "server/system_message" => game.system_message(argument),
        "allowlist" => Ok(lists.allowlist()),
        "bans" => Ok(lists.bans()),
        "ip_bans" => Ok(lists.ip_bans()),
        "operators" => Ok(lists.operators()),
        _ => Err((
            INTERNAL_ERROR,
            Some(format!("minecraft:{short} isn't in Horizon yet")),
        )),
    }
}

/// `UUIDUtil` writes a UUID as 8-4-4-4-12 lowercase hex digits.
fn uuid(id: u128) -> String {
    let h = format!("{id:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// A `PlayerDto` from a request: an id, a name, or both.
fn player_dto(value: &Value) -> Result<(Option<u128>, Option<String>), Failure> {
    let Some(object) = value.as_object() else {
        return Err(invalid_params("Not a player"));
    };
    let id = match object.get("id") {
        None => None,
        Some(id) => Some(
            id.as_str()
                .and_then(|s| u128::from_str_radix(&s.replace('-', ""), 16).ok())
                .ok_or_else(|| invalid_params(format!("Invalid UUID: {id}")))?,
        ),
    };
    let name = match object.get("name") {
        None => None,
        Some(name) => Some(
            name.as_str()
                .ok_or_else(|| invalid_params("Not a string: name"))?
                .to_owned(),
        ),
    };
    Ok((id, name))
}

/// `Message.asComponent` as a JSON text component; `None` when it has neither text nor key.
fn component(message: &Value) -> Option<Value> {
    if let Some(key) = message["translatable"].as_str() {
        let mut c = json!({ "translate": key });
        if let Some(with) = message["translatableParams"].as_array() {
            c["with"] = Value::Array(with.clone());
        }
        return Some(c);
    }
    message["literal"]
        .as_str()
        .map(|text| json!({ "text": text }))
}

/// The running game, through its control endpoint.
struct Game<'a> {
    shared: &'a Shared,
}

impl Game<'_> {
    fn running(&self) -> bool {
        self.shared.state.lock().unwrap().phase == Phase::Running
    }

    fn ask(&self, line: &str) -> Result<Value, Failure> {
        engine::Control::connect(&self.shared.dir)
            .and_then(|mut game| game.ask(line))
            .map_err(|e| (INTERNAL_ERROR, Some(format!("the game didn't answer: {e}"))))
    }

    fn console(&self, line: &str) -> Result<Value, Failure> {
        self.ask(&format!("console {line}"))
    }

    /// `PlayerService.get`: the players online, as `PlayerDto`s.
    fn online(&self) -> Result<Vec<Value>, Failure> {
        let status = self.ask("server-status")?;
        Ok(status["players"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| json!({ "id": p["uuid"], "name": p["name"] }))
            .collect())
    }

    /// An online player's name, by id first, as `PlayerService.getServerPlayer` finds them.
    fn find(&self, online: &[Value], dto: &Value) -> Result<Option<String>, Failure> {
        let (id, name) = player_dto(dto)?;
        let found = online.iter().find(|p| match (id, &name) {
            (Some(id), _) => p["id"] == uuid(id),
            (None, Some(name)) => p["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name)),
            (None, None) => false,
        });
        Ok(found.and_then(|p| p["name"].as_str().map(str::to_owned)))
    }

    /// `ServerStateService.status`.
    fn status(&self) -> Value {
        let version = json!({
            "name": horizon_core::version::VERSION_ID,
            "protocol": horizon_core::version::PROTOCOL_VERSION,
        });
        let players = if self.running() {
            self.online().ok()
        } else {
            None
        };
        let Some(players) = players else {
            return json!({ "started": false, "version": version });
        };
        let mut state = json!({ "started": true, "version": version });
        // An empty list is left out, as vanilla's codec leaves out its default.
        if !players.is_empty() {
            state["players"] = Value::Array(players);
        }
        state
    }

    /// `PlayerService.kick`: the players found, as they were asked for.
    fn kick(&self, argument: &Value) -> Result<Value, Failure> {
        let kicks = argument
            .as_array()
            .ok_or_else(|| invalid_params("Not a list"))?;
        let online = self.online()?;
        let mut kicked = Vec::new();
        for kick in kicks {
            let Some(name) = self.find(&online, &kick["player"])? else {
                continue;
            };
            let reason = kick.get("message").and_then(component).map(|c| {
                c["text"]
                    .as_str()
                    .or(c["translate"].as_str())
                    .unwrap_or_default()
                    .to_owned()
            });
            let line = match reason {
                Some(reason) if !reason.is_empty() => format!("kick {name} {reason}"),
                _ => format!("kick {name}"),
            };
            self.console(&line)?;
            kicked.push(kick["player"].clone());
        }
        Ok(Value::Array(kicked))
    }

    /// `ServerStateService.systemMessage`: to everyone, or to the players named (those online).
    fn system_message(&self, argument: &Value) -> Result<Value, Failure> {
        if argument["overlay"].as_bool().is_none() {
            return Err(invalid_params("No key overlay"));
        }
        let Some(text) = component(&argument["message"]) else {
            return Ok(json!(false));
        };
        let targets = match argument.get("receivingPlayers").and_then(Value::as_array) {
            Some(players) if players.is_empty() => return Ok(json!(false)),
            Some(players) => {
                let online = self.online()?;
                let mut names = Vec::new();
                for p in players {
                    names.extend(self.find(&online, p)?);
                }
                names
            }
            None => vec!["@a".to_owned()],
        };
        for target in targets {
            if argument["overlay"] == true {
                self.console(&format!("title {target} actionbar {text}"))?;
            } else {
                self.console(&format!("tellraw {target} {text}"))?;
            }
        }
        Ok(json!(true))
    }
}

/// Vanilla's lists in the server folder, as the game keeps them.
struct Lists<'a>(&'a Path);

impl Lists<'_> {
    fn load<E: horizon_server_files::user_lists::Entry>(
        &self,
        file: &str,
    ) -> horizon_server_files::user_lists::UserList<E> {
        let mut list = horizon_server_files::user_lists::UserList::new(self.0.join(file));
        let _ = list.load();
        list
    }

    fn allowlist(&self) -> Value {
        let list: horizon_server_files::user_lists::WhiteList = self.load("whitelist.json");
        list.entries()
            .iter()
            .map(|e| json!({ "id": uuid(e.user.id), "name": e.user.name }))
            .collect()
    }

    fn operators(&self) -> Value {
        let list: horizon_server_files::user_lists::OpList = self.load("ops.json");
        list.entries()
            .iter()
            .map(|e| {
                json!({
                    "player": { "id": uuid(e.user.id), "name": e.user.name },
                    "permissionLevel": e.level,
                    "bypassesPlayerLimit": e.bypasses_player_limit,
                })
            })
            .collect()
    }

    fn bans(&self) -> Value {
        let list: horizon_server_files::user_lists::BanList = self.load("banned-players.json");
        list.entries()
            .iter()
            .map(|b| {
                let player = json!({ "id": uuid(b.user.id), "name": b.user.name });
                ban(("player", player), &b.reason, &b.source, b.expires)
            })
            .collect()
    }

    fn ip_bans(&self) -> Value {
        let list: horizon_server_files::user_lists::IpBanList = self.load("banned-ips.json");
        list.entries()
            .iter()
            .map(|b| ban(("ip", json!(b.user)), &b.reason, &b.source, b.expires))
            .collect()
    }
}

/// A `UserBanDto` or `IpBanDto`: the reason when there is one, the end as an ISO-8601 instant.
fn ban(who: (&str, Value), reason: &Option<String>, source: &str, expires: Option<i64>) -> Value {
    let mut b = json!({ "source": source });
    b[who.0] = who.1;
    if let Some(reason) = reason {
        b["reason"] = json!(reason);
    }
    if let Some(at) = expires.and_then(chrono::DateTime::from_timestamp_millis) {
        b["expires"] = json!(at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true));
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> std::sync::Arc<Shared> {
        let dir = std::env::temp_dir().join(format!(
            "horizon-management-methods-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Shared::with_install(&dir, None)
    }

    #[test]
    fn the_lists_read_as_vanillas_dtos() {
        let shared = shared();
        let dir = &shared.dir;
        std::fs::write(
            dir.join("banned-players.json"),
            r#"[{"uuid":"93e029a5-04a0-349d-90b2-f7e145b2644f","name":"Kim0","created":"2026-10-31 14:00:00 +0000",
                "source":"Management server","expires":"2026-11-01 00:00:00 +0000","reason":"griefing"}]"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("ops.json"),
            r#"[{"uuid":"93e029a5-04a0-349d-90b2-f7e145b2644f","name":"Kim0","level":3,"bypassesPlayerLimit":true}]"#,
        )
        .unwrap();
        let lists = Lists(dir);
        assert_eq!(
            lists.bans(),
            json!([{ "player": { "id": "93e029a5-04a0-349d-90b2-f7e145b2644f", "name": "Kim0" },
                     "source": "Management server", "reason": "griefing", "expires": "2026-11-01T00:00:00Z" }])
        );
        assert_eq!(lists.operators()[0]["permissionLevel"], 3);
        assert_eq!(lists.operators()[0]["bypassesPlayerLimit"], true);
        assert_eq!(lists.allowlist(), json!([]));
        assert_eq!(lists.ip_bans(), json!([]));
    }

    #[test]
    fn the_status_says_when_the_game_isnt_up() {
        let shared = shared();
        let reply = crate::management::receive(
            &shared,
            r#"{"jsonrpc":"2.0","id":null,"method":"server/status"}"#,
        );
        assert_eq!(
            reply.unwrap().unwrap(),
            json!({ "jsonrpc": "2.0", "id": null, "result": {
                "started": false, "version": { "name": "26.3", "protocol": 777 } } })
        );
    }
}
