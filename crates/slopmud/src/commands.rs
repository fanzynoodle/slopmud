use std::collections::HashMap;
use std::sync::Arc;

use mudproto::session::SessionId;
use tokio::sync::Mutex;

use crate::SessionInfo;

pub async fn handle_whoami_command(
    sessions: &Arc<Mutex<HashMap<SessionId, SessionInfo>>>,
    session: SessionId,
) -> String {
    let si = {
        let m = sessions.lock().await;
        m.get(&session).cloned()
    };
    let Some(si) = si else {
        return "whoami: not attached\r\n> ".to_string();
    };

    let mut s = String::new();
    s.push_str("whoami:\r\n");
    s.push_str(&format!(" - name: {}\r\n", si.name));
    s.push_str(&format!(" - race: {}\r\n", si.race));
    s.push_str(&format!(" - class: {}\r\n", si.class));
    s.push_str(&format!(" - sex: {}\r\n", si.sex));
    s.push_str(&format!(" - pronouns: {}\r\n", si.pronouns));
    s.push_str(&format!(
        " - role: {}\r\n",
        if si.is_bot { "bot" } else { "player" }
    ));
    s.push_str(&format!(
        " - held: {}\r\n",
        if si.held { "yes" } else { "no" }
    ));
    s.push_str(&format!(" - peer_ip: {}\r\n", si.peer_ip));
    s.push_str("\r\n> ");
    s
}

pub async fn handle_who_command(sessions: &Arc<Mutex<HashMap<SessionId, SessionInfo>>>) -> String {
    let names = {
        let m = sessions.lock().await;
        let mut out = m
            .values()
            .map(|si| {
                if si.is_bot {
                    format!("{} [bot]", si.name)
                } else {
                    si.name.clone()
                }
            })
            .collect::<Vec<_>>();
        out.sort_unstable();
        out
    };

    let mut s = String::new();
    s.push_str("who:\r\n");
    if names.is_empty() {
        s.push_str(" - (none)\r\n");
    } else {
        for name in names {
            s.push_str(&format!(" - {name}\r\n"));
        }
    }
    s.push_str("\r\n> ");
    s
}
