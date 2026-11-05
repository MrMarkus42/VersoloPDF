//! "Vorlesen": text to speech with the speech output of the operating system
//! (Windows: SAPI through PowerShell's System.Speech, macOS: `say`, Linux:
//! `spd-say` or `espeak-ng`). No extra library is needed. The text is passed
//! through standard input, never on a command line.

use std::io::Write;
use std::process::{Child, Command, Stdio};

use crate::{Error, Result};

/// Upper bound of the text read in one go (characters).
pub const MAX_TEXT: usize = 200_000;

/// A running speech output; dropping it stops the speech.
pub struct Reader {
    child: Child,
    /// Page being read (None: a selection).
    pub page: Option<usize>,
    /// Pages still to read after this one ("Bis zum Ende vorlesen").
    pub to_end: bool,
}

/// Command lines tried in this order, with the text going to stdin.
fn commands(volume: u32, rate: i32) -> Vec<Command> {
    let volume = volume.min(100);
    let rate = rate.clamp(-10, 10);
    let mut list = Vec::new();
    if cfg!(windows) {
        // Numbers only are put into the script; the text comes via stdin as UTF-8.
        let script = format!(
            "[Console]::InputEncoding = [Text.Encoding]::UTF8; Add-Type -AssemblyName System.Speech; \
             $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Volume = {volume}; \
             $s.Rate = {rate}; $s.Speak([Console]::In.ReadToEnd())"
        );
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            c.creation_flags(CREATE_NO_WINDOW);
        }
        list.push(c);
    } else if cfg!(target_os = "macos") {
        // The voice is the one chosen in the system settings (Bedienungshilfen
        // › Gesprochene Inhalte).
        let mut c = Command::new("/usr/bin/say");
        c.arg("-r").arg((175 + rate * 15).to_string());
        list.push(c);
    } else {
        let mut c = Command::new("spd-say");
        c.args(["-e", "-w", "-r"])
            .arg((rate * 10).to_string())
            .arg("-i")
            .arg((volume as i32 * 2 - 100).to_string());
        list.push(c);
        let mut c = Command::new("espeak-ng");
        c.args(["--stdin", "-a"]).arg((volume * 2).to_string()).arg("-s").arg((175 + rate * 15).to_string());
        list.push(c);
    }
    list
}

/// Prepares text for speaking: no control characters, limited length.
pub fn clean_text(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .take(MAX_TEXT)
        .collect::<String>()
        .trim()
        .to_string()
}

impl Reader {
    /// Starts reading `text` aloud.
    pub fn start(text: &str, volume: u32, rate: i32) -> Result<Reader> {
        let mut text = clean_text(text);
        if cfg!(target_os = "macos") {
            // `say` takes the volume as a command inside the text; commands
            // in the document's own text are disarmed.
            text = format!("[[volm {:.2}]] {}", volume.min(100) as f32 / 100.0, text.replace("[[", "[ ["));
        }
        let mut last_err = None;
        for mut cmd in commands(volume, rate) {
            cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
            match cmd.spawn() {
                Ok(mut child) => {
                    if let Some(mut stdin) = child.stdin.take() {
                        // Written from a thread so a slow reader never blocks the UI.
                        let bytes = text.into_bytes();
                        std::thread::spawn(move || {
                            let _ = stdin.write_all(&bytes);
                        });
                    }
                    return Ok(Reader { child, page: None, to_end: false });
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(Error::Invalid(match last_err {
            Some(e) => format!("speech output not available: {e}"),
            None => "speech output not available".into(),
        }))
    }

    /// True while the speech is still running.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_cleaned_and_limited() {
        assert_eq!(clean_text("  a\u{7}b\nc  "), "a b\nc");
        assert_eq!(clean_text(&"x".repeat(MAX_TEXT + 10)).len(), MAX_TEXT);
    }

    #[test]
    fn command_values_are_limited() {
        // Only numbers within range reach the command line.
        for cmd in commands(500, 99) {
            let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
            let joined = args.join(" ");
            assert!(!joined.contains("500") && !joined.contains("99"), "{joined}");
        }
    }
}
