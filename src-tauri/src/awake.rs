// Holds an OS sleep lock while an agent turn runs; dropping releases it.
// Display may sleep.
pub type Guard = keepawake::KeepAwake;

pub fn acquire() -> Option<Guard> {
    match keepawake::Builder::default()
        .idle(true)
        .sleep(true)
        .display(false)
        .reason("Agent turn running")
        .app_name("Samokod")
        .app_reverse_domain("app.samokod.desktop")
        .create()
    {
        Ok(guard) => Some(guard),
        Err(error) => {
            log::warn!("failed to inhibit sleep: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_never_panics() {
        let _ = acquire();
    }
}
