//! What the surfaces show: faders, V-Pot rings, lights, the LCD and the digits, sent where they
//! differ from what a unit shows (see `mod.rs`).

use super::*;

impl Session {
    /// Sends each unit what changed in what it should show.
    pub(super) fn surface_show(&mut self, now: Instant) {
        let tracks = self.surface_tracks();
        let bank = self.surface_bank(tracks.len());
        self.surfaces.bank = bank;
        let outputs = self.midi_out.keys();
        let frames: Vec<Option<Frame>> =
            self.surfaces.units.iter().map(|u| outputs.contains(&u.cfg.output).then(|| self.surface_frame(u, &tracks, bank, now))).collect();
        let Surfaces { units, out, .. } = &mut self.surfaces;
        for (u, frame) in units.iter_mut().zip(frames) {
            let Some(f) = frame else { continue };
            let port = u.cfg.output.clone();
            let mut send = |m: Vec<u8>| out.push((port.clone(), m));
            if !u.hui() && !u.meter_mode {
                u.meter_mode = true;
                (0..STRIPS as u8).for_each(|s| send(mcu::meter_mode(u.device(), s, METER_MODE)));
            }
            for (s, v) in f.faders.iter().enumerate() {
                let Some(v) = *v else { continue };
                if u.touched[s] {
                    continue;
                }
                let msgs = u.fader(s as u8, v);
                if let Some(ms) = u.faders[s].due(msgs, now) {
                    ms.into_iter().for_each(|m| send(m.to_vec()));
                }
            }
            for (b, on) in f.leds {
                if u.leds.get(&b) == Some(&on) {
                    continue;
                }
                u.leds.insert(b, on);
                if u.hui() {
                    hui::led(b, on).into_iter().flatten().for_each(|m| send(m.to_vec()));
                } else if let Some(m) = mcu::led(b, on) {
                    send(m.to_vec());
                }
            }
            for (s, v) in f.rings.iter().enumerate() {
                if u.rings[s] != Some(*v) {
                    u.rings[s] = Some(*v);
                    send(if u.hui() { hui::ring(s as u8, *v) } else { mcu::ring(s as u8, *v) }.to_vec());
                }
            }
            if u.lcd.as_ref() != Some(&f.lcd) {
                if u.hui() {
                    for (s, cell) in f.lcd.chunks(4).enumerate() {
                        if u.lcd.as_ref().is_none_or(|old| old.get(s * 4..s * 4 + 4) != Some(cell)) {
                            send(hui::scribble(s as u8, cell));
                        }
                    }
                } else {
                    for (offset, text) in lcd_changes(u.lcd.as_deref(), &f.lcd) {
                        send(mcu::lcd(u.device(), offset as u8, text));
                    }
                }
                u.lcd = Some(f.lcd);
            }
            // The digits, rightmost first as both protocols count them.
            let digits: Vec<Digit> = f.digits.iter().rev().copied().collect();
            if u.hui() {
                if !digits.is_empty() && (u.digits.len() != digits.len() || u.digits.iter().zip(&digits).any(|(a, b)| *a != Some(*b))) {
                    send(hui::timecode(&f.digits));
                    u.digits = digits.into_iter().map(Some).collect();
                }
            } else {
                u.digits.resize(digits.len(), None);
                for (i, d) in digits.iter().enumerate() {
                    if u.digits[i] != Some(*d) {
                        u.digits[i] = Some(*d);
                        // The assignment display's two come first in `digits`' last two.
                        send(mcu::digit(i as u8, *d).to_vec());
                    }
                }
            }
            for (s, level) in f.meters.iter().enumerate() {
                let (last, at) = u.meters[s];
                let again = *level > 0 && at.is_none_or(|t| now.saturating_duration_since(t) >= METER_EVERY);
                if *level != last || again || at.is_none() {
                    u.meters[s] = (*level, Some(now));
                    if u.hui() {
                        hui::meter(s as u8, *level).into_iter().for_each(|m| send(m.to_vec()));
                    } else {
                        send(mcu::meter(s as u8, *level).to_vec());
                    }
                }
            }
        }
    }

    /// What unit `u` should show, with the bank at `bank` of `tracks`.
    fn surface_frame(&self, u: &Unit, tracks: &[Id], bank: usize, now: Instant) -> Frame {
        let sfs = &self.surfaces;
        let travel = |t: &CtlTarget| match self.ctl_shown(t) {
            Some(Shown::Travel(v)) => Some(v),
            Some(Shown::Switch(on)) => Some(on as u8 as f32),
            None => None,
        };
        let recent = |at: Option<&Instant>| at.is_some_and(|t| now.saturating_duration_since(*t) < SHOW_VALUE);
        let params = if sfs.pot == PotMode::Plugin { self.surface_params() } else { None };
        let width = if u.hui() { 4 } else { CELL - 1 };
        let mut f = Frame { faders: [None; STRIPS + 1], leds: Vec::new(), rings: [0; STRIPS], lcd: Vec::new(), digits: Vec::new(), meters: [0; STRIPS] };
        let (mut top, mut bottom) = (Vec::new(), Vec::new());
        for s in 0..STRIPS {
            let place = u.cfg.offset as usize + s;
            let track = tracks.get(bank + place).copied();
            let t = track.and_then(|id| self.project.track(id));
            f.faders[s] = Some(track.and_then(|track| travel(&CtlTarget::Volume { track })).unwrap_or(0.0));
            let s8 = s as u8;
            f.leds.push((Button::Rec(s8), t.is_some_and(|t| t.armed)));
            f.leds.push((Button::Solo(s8), t.is_some_and(|t| t.solo)));
            f.leds.push((Button::Mute(s8), t.is_some_and(|t| t.mute)));
            f.leds.push((Button::Select(s8), track.is_some() && track == self.selected));
            f.meters[s] = track.and_then(|id| sfs.peaks.get(&id)).map(|p| sf::meter_level(*p)).unwrap_or(0);
            // The V-Pot: its ring, and the strip's two lines.
            let pot = self.surface_pot(place, track);
            let resolved = pot.as_ref().and_then(|p| self.ctl_resolve(p));
            let v = resolved.as_ref().and_then(travel);
            let mode = if sfs.pot == PotMode::Pan { RingMode::Dot } else { RingMode::Wrap };
            f.rings[s] = sf::ring_value(mode, v, false);
            let param = match (&resolved, &params) {
                (Some(CtlTarget::Plugin { param, .. }), Some(ps)) => ps.iter().find(|p| p.id == *param),
                _ => None,
            };
            let name = match (sfs.pot, param) {
                (PotMode::Plugin, Some(p)) => p.name.clone(),
                (PotMode::Plugin, None) => String::new(),
                _ => t.map(|t| t.name.clone()).unwrap_or_default(),
            };
            let value = match (&resolved, v) {
                (Some(CtlTarget::Pan { .. }), Some(v)) => sf::pan_text(v * 2.0 - 1.0),
                (Some(CtlTarget::Send { .. }), Some(v)) => sf::db_text(ctl::fader_to_db(v)),
                (Some(_), Some(v)) => sf::percent_text(v),
                _ => String::new(),
            };
            let label = match (sfs.pot, param) {
                (PotMode::Pan, _) => "Pan".to_string(),
                (PotMode::Send(n), _) => format!("Send{}", n + 1),
                (PotMode::Plugin, p) => p.map(|p| p.name.clone()).unwrap_or_default(),
            };
            let level = track.filter(|id| recent(sfs.fader_moved.get(id)) || u.touched[s]).and_then(|id| travel(&CtlTarget::Volume { track: id }));
            let line2 = if let Some(l) = level {
                sf::db_text(ctl::fader_to_db(l))
            } else if sfs.names && !recent(sfs.pot_moved.get(&place)) && pot.is_some() {
                label
            } else {
                value
            };
            if u.hui() {
                // Four characters: the name, or a value while it moves.
                let moving = level.is_some() || recent(sfs.pot_moved.get(&place));
                let text = if moving { short_value(&line2) } else { sf::fit(&name, width) };
                top.extend(sf::center(&text, width).bytes());
            } else {
                top.extend(sf::fit(&name, width).bytes());
                top.push(b' ');
                bottom.extend(sf::center(&line2, width).bytes());
                bottom.push(b' ');
            }
        }
        if u.has_master() {
            f.faders[MASTER as usize] = travel(&CtlTarget::MasterVolume);
        }
        f.lcd = top;
        if u.hui() {
            let assign = match sfs.pot {
                PotMode::Pan => "Pan".to_string(),
                PotMode::Send(n) => format!("Snd{}", n + 1),
                PotMode::Plugin => "Plug".into(),
            };
            f.lcd.extend(sf::fit(&assign, 4).bytes());
        } else {
            f.lcd.extend(bottom);
        }
        if u.cfg.kind.is_main() {
            self.surface_main(u, &mut f);
        }
        f
    }

    /// The master section: its lights and the time display.
    fn surface_main(&self, u: &Unit, f: &mut Frame) {
        let sfs = &self.surfaces;
        let playing = self.is_playing();
        let mode = self.selected.and_then(|t| self.auto_mode_of(t));
        let any_solo = self.project.tracks.iter().any(|t| t.solo);
        f.leds.extend([
            (Button::Play, playing),
            (Button::Stop, !playing),
            (Button::Record, self.is_recording()),
            (Button::Cycle, self.project.cycle.enabled),
            (Button::Click, self.project.metronome),
            (Button::Replace, self.prefs.recording.replace),
            (Button::Drop, self.project.punch.enabled),
            (Button::AssignPan, sfs.pot == PotMode::Pan),
            (Button::AssignSend, matches!(sfs.pot, PotMode::Send(_))),
            (Button::AssignPlugin, sfs.pot == PotMode::Plugin),
            (Button::Read, mode == Some(AutoMode::Read)),
            (Button::Write, mode == Some(AutoMode::Write)),
            (Button::Touch, mode == Some(AutoMode::Touch)),
            (Button::Latch, mode == Some(AutoMode::Latch)),
            (Button::Marker, sfs.held.contains(&Button::Marker)),
            (Button::Nudge, sfs.nudge),
            (Button::Zoom, sfs.zoom),
            (Button::Scrub, sfs.scrub),
            (Button::NameValue, sfs.names),
            (Button::SmpteLight, sfs.time == TimeMode::Time),
            (Button::BeatsLight, sfs.time == TimeMode::Bars),
            (Button::SoloClear, any_solo),
            (Button::RudeSolo, any_solo),
        ]);
        let pos = self.position().max(0) as f64;
        let secs = pos / self.sample_rate().max(1.0);
        let bbt = || self.tempo_map().bar_beat_at(pos);
        f.digits = match (u.hui(), sfs.time) {
            (true, TimeMode::Bars) => sf::hui_bbt_digits(&bbt()),
            (true, TimeMode::Time) => sf::hui_time_digits(secs),
            (false, TimeMode::Bars) => sf::bbt_digits(&bbt()),
            (false, TimeMode::Time) => sf::time_digits(secs),
        };
        if !u.hui() {
            // The assignment display, left of the time display: what the V-Pots drive.
            let assign = match sfs.pot {
                PotMode::Pan => "PN".to_string(),
                PotMode::Send(n) => format!("S{}", (n + 1) % 10),
                PotMode::Plugin => "PI".into(),
            };
            let mut digits: Vec<Digit> = assign.bytes().map(|ch| Digit { ch, dot: false }).collect();
            digits.append(&mut f.digits);
            f.digits = digits;
        }
    }
}

/// A value in a HUI's four characters: "-12.5" as "-12", "-inf" as "-oo".
fn short_value(text: &str) -> String {
    if text.chars().count() <= 4 {
        return text.to_string();
    }
    match text.parse::<f32>() {
        Ok(v) => format!("{:+.0}", v),
        Err(_) => text.chars().take(4).collect(),
    }
}

/// The stretches of a Mackie Control's LCD that differ from what it shows, one per line:
/// (offset, characters). Everything when what it shows is unknown.
fn lcd_changes<'a>(old: Option<&[u8]>, new: &'a [u8]) -> Vec<(usize, &'a [u8])> {
    let line = new.len() / 2;
    let mut out = Vec::new();
    for start in [0, line] {
        let end = (start + line).min(new.len());
        let differs = |i: usize| old.is_none_or(|o| o.get(i) != Some(&new[i]));
        let Some(first) = (start..end).find(|&i| differs(i)) else { continue };
        let last = (start..end).rev().find(|&i| differs(i)).unwrap_or(first);
        out.push((first, &new[first..=last]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcd_sends_what_changed() {
        let mut a = vec![b' '; 112];
        a[..12].copy_from_slice(b"Kick   Snare");
        a[56..66].copy_from_slice(b"  C      C");
        let all = lcd_changes(None, &a);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0, 0);
        assert_eq!(all[1].0, a.len() / 2);
        assert_eq!(lcd_changes(Some(&a), &a), vec![]);
        let mut b = a.clone();
        b[7..12].copy_from_slice(b"Clap ");
        let line = a.len() / 2;
        b[line + 7] = b'L';
        assert_eq!(lcd_changes(Some(&a), &b), vec![(7, &b"Clap "[..]), (line + 7, &b"L"[..])]);
    }

    #[test]
    fn hui_values_fit_four_characters() {
        assert_eq!(short_value("C"), "C");
        assert_eq!(short_value("L32"), "L32");
        assert_eq!(short_value("-12.5"), "-12");
        assert_eq!(short_value("+3.0"), "+3.0");
        assert_eq!(short_value("-inf"), "-inf");
        assert_eq!(short_value("100%"), "100%");
    }
}
