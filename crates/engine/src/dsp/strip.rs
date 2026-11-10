//! One strip's work for one block ([`run_strip`]): regions or instrument, Channel EQ, inserts,
//! automation, fader, sends and output. The serial loop and the worker threads run this same code;
//! [`Routes`] says how the strip reaches other strips.
//!
//! Runs on the audio thread, or on one of the pool's real-time workers: it must not allocate.

use super::*;

/// How `process_block` ended.
pub(super) enum BlockEnd {
    Played,
    /// A strip on the pool panicked: the block's audio is no good.
    Silenced,
    /// A strip on the pool hasn't finished (see `Parallel::run`).
    GivenUp,
    /// Strips on the pool were left in plug-ins that didn't return, and the rest finished.
    LeftOut,
}

/// One strip's work for the block. The serial loop and the worker threads run this same code, with
/// `routes` saying how the strip reaches other strips (see [`Routes`]).
pub(super) fn run_strip<R: Routes>(cx: &BlockCtx<'_>, ti: usize, t: &mut TrackDsp, sc: &mut Scratch, routes: &mut R) {
    let BlockCtx {
        n,
        pos,
        playing,
        recording,
        sr,
        sources,
        punch_range,
        capture_from,
        capture_end,
        auto_monitor,
        low_latency,
        block_end,
        smooth,
        any_solo,
        snap_gains,
        iso,
        ..
    } = *cx;
    t.instrument_ran = false;
    let role = iso.map(|i| i.role(t.id));
    if role == Some(Role::Skip) {
        return;
    }
    // Its plug-in stopped responding, or it's fed by one that did: it plays nothing, and what's
    // sent to it is let go.
    if t.left_out {
        t.sum_l[..n].fill(0.0);
        t.sum_r[..n].fill(0.0);
        if cx.analyzer_target.is_some_and(|a| a.0 == t.id) {
            routes.analyze(&mut t.outs, None, n);
        }
        return;
    }
    // An Off track goes quiet through its fader as a muted one does, then stops. A track
    // in the middle of a take stays on until the take ends (only an undo can get here:
    // the session refuses the switch while recording), so no audio is lost.
    let taking = recording && t.params.armed && (t.rec.is_some() || cx.midi_target == Some(t.id));
    let off = t.params.off && !taking;
    if off && (t.dormant || snap_gains || iso.is_some() || t.faded_out()) {
        // It may call its plug-ins, though only for the strip itself.
        routes.enter(0);
        t.sleep_block(n, &cx.transport);
        if !routes.leave() {
            return;
        }
        if cx.analyzer_target.is_some_and(|a| a.0 == t.id) {
            routes.analyze(&mut t.outs, None, n);
        }
        return;
    }
    if t.dormant {
        t.wake(if snap_gains { 0 } else { track_off::resume_frames(sr) });
    }
    let tap = role == Some(Role::Tap);
    let fx = !tap || iso.map(|i| i.effects).unwrap_or(true);
    let (l, r) = (&mut t.l[..n], &mut t.r[..n]);
    l.fill(0.0);
    r.fill(0.0);
    t.midi.clear();
    let mut p = t.params;
    let pdc = t.pdc;
    // A live track (recording, or monitored in Low Latency Mode) skips its delay
    // compensation, so the performer isn't kept waiting (see session/engine/pdc.rs). An aux on an
    // instrument's extra output is monitored with the instrument, so it keeps up with the
    // instrument's main output.
    let monitored = match t.kind {
        TrackKind::Audio => p.monitor || (p.armed && auto_monitor),
        TrackKind::Instrument => p.armed,
        TrackKind::Aux if p.instrument_output.is_some() => t.feed.source.and_then(|i| routes.earlier(i)).is_some_and(|s| s.params.armed),
        _ => false,
    };
    let live = (recording && p.armed && t.kind == TrackKind::Audio) || (low_latency && monitored);
    let ll_skip = if low_latency && monitored { pdc.low_latency_skip } else { 0 };
    // The strip's plug-ins hear the timeline `input_time` late, and its fader
    // `output_time` late: automation and the plug-ins' transport follow the audio.
    let fx_at = pos - pdc.input_time as i64;
    let mix_at = pos - pdc.output_time as i64;
    // A folder's lanes are read where this strip's fader is, like its own.
    if let Some(j) = t.fold_idx.filter(|&j| j > ti) {
        auto::apply_fold(&mut p, routes.fold(t.fold_now, j, mix_at, playing));
    }
    let transport = if pdc.input_time > 0 { transport_at(cx.tempo, &cx.transport, fx_at) } else { cx.transport };

    // Automation overrides the fader, pan and mute (and drives plug-in parameters).
    // Write mode plays the live values, which are being recorded over the lanes.
    if playing && matches!(t.auto_mode, AutoMode::Read | AutoMode::Touch | AutoMode::Latch) {
        for (k, lane) in t.automation.iter().enumerate() {
            if t.touched.contains(&Some(lane.target)) {
                continue;
            }
            let at = if matches!(lane.target, AutoTarget::Plugin { .. }) { fx_at } else { mix_at };
            let last = at + n as i64 - 1;
            let Some(v) = value_at(&lane.points, at) else { continue };
            match lane.target {
                AutoTarget::Volume => p.volume = volume_gain(v) * p.stack_gain,
                AutoTarget::Pan => p.pan = v,
                AutoTarget::Mute => p.mute = v >= 0.5 || p.stack_mute,
                // A muted send (gain 0) stays silent.
                AutoTarget::Send(slot) => {
                    if let Some(Some(s)) = p.sends.get_mut(slot as usize) {
                        if s.gain > 0.0 {
                            s.gain = volume_gain(v);
                        }
                    }
                }
                AutoTarget::Plugin { uid, param } => {
                    // Two points per block: the plug-in ramps between them.
                    let v1 = value_at(&lane.points, last).unwrap_or(v);
                    let sent = t.auto_sent.get(k).copied().unwrap_or(f32::NAN);
                    if v == sent && v1 == sent {
                        continue;
                    }
                    let slot = t.inserts.iter_mut().flatten().chain(t.instrument.iter_mut()).find(|i| i.uid == uid);
                    if let Some(ins) = slot {
                        ins.plugin.set_param(param, 0, v as f64);
                        if v1 != v {
                            ins.plugin.set_param(param, n as i32 - 1, v1 as f64);
                        }
                    }
                    if let Some(s) = t.auto_sent.get_mut(k) {
                        *s = v1;
                    }
                }
            }
        }
    }

    match t.kind {
        // A folder only shapes its subtracks' settings (worked out by the session).
        TrackKind::Folder => return,
        TrackKind::Summing | TrackKind::Aux => {
            routes.inputs((&mut t.sum_l[..n], &mut t.sum_r[..n]), l, r);
            if let Some(o) = p.instrument_output {
                let delay = if live { 0 } else { pdc.feed };
                let pull = multi_out::Pull { bus: o.bus as usize, smooth, snap: snap_gains, delay, fade: (pdc::FADE_SECS * sr) as u32 };
                let src = t.feed.source.and_then(|i| routes.earlier(i));
                t.feed.pull(src, &pull, &mut t.pdc_taps.feed, (l, r), (&mut sc.tap_l[..n], &mut sc.tap_r[..n]));
                if contain(l, r) {
                    cx.status.bad_audio.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        TrackKind::Audio => {
            // A track being recorded is silent, or with Autopunch only inside the range.
            let quiet = match (recording && p.armed, punch_range) {
                (false, _) => (0, 0),
                (true, Some(r)) => r,
                (true, None) => (i64::MIN, i64::MAX),
            };
            if sources && !(quiet.0 <= pos && quiet.1 >= block_end) {
                play_audio(&t.regions, pos, l, r, quiet);
            }
            // Input is heard with the I button, or on armed tracks when auto monitoring is
            // on. Interfaces with direct monitoring want it off to avoid hearing a
            // delayed second copy.
            if p.monitor || (p.armed && auto_monitor) {
                match p.input {
                    InputSel::Mono(c) if (c as usize) < MAX_INPUTS => {
                        let src = &cx.inputs[c as usize][..n];
                        for i in 0..n {
                            l[i] += src[i];
                            r[i] += src[i];
                        }
                    }
                    InputSel::Stereo(c) if (c as usize + 1) < MAX_INPUTS => {
                        for i in 0..n {
                            l[i] += cx.inputs[c as usize][i];
                            r[i] += cx.inputs[c as usize + 1][i];
                        }
                    }
                    _ => {}
                }
            }
            if recording && p.armed && block_end > capture_from && pos < capture_end {
                if let Some(sink) = &mut t.rec {
                    // The capture bounds can be the i64 extremes ("no punch-out"), and
                    // `pos` is negative during a count-in from the start, so plain
                    // subtraction would overflow.
                    let first = capture_from.saturating_sub(pos).clamp(0, n as i64) as usize;
                    let last = capture_end.saturating_sub(pos).clamp(0, n as i64) as usize;
                    let mut lost = false;
                    for i in first..last {
                        match p.input {
                            InputSel::Mono(c) => lost |= sink.push(cx.inputs[c as usize % MAX_INPUTS][i]).is_err(),
                            InputSel::Stereo(c) => {
                                let c = c as usize % (MAX_INPUTS - 1);
                                lost |= sink.push(cx.inputs[c][i]).is_err();
                                lost |= sink.push(cx.inputs[c + 1][i]).is_err();
                            }
                            InputSel::None => {}
                        }
                    }
                    if lost {
                        cx.status.overruns.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        TrackKind::Instrument => {
            if t.flush_notes {
                for pitch in 0..128 {
                    if t.sounding[pitch] {
                        t.midi.push(MidiEvent { offset: 0, data: [0x80, pitch as u8, 0] });
                        t.sounding[pitch] = false;
                    }
                }
                if t.cc_sent {
                    // Don't leave the pedal down or the pitch bent after a stop.
                    t.midi.push(MidiEvent { offset: 0, data: [0xB0, 64, 0] });
                    t.midi.push(MidiEvent { offset: 0, data: [0xE0, 0, 64] });
                    t.cc_sent = false;
                }
                if let Some(InsertDsp { plugin: Plugin::Instrument(i), .. }) = &mut t.instrument {
                    i.all_notes_off();
                }
                t.flush_notes = false;
            }
            if sources && t.chase_cc {
                t.chase_cc = false;
                // The latest value of each controller before the playhead, in the
                // region that covers it.
                let mut last: [Option<u16>; CC_KINDS] = [None; CC_KINDS];
                for reg in t.regions.iter().filter(|_| cx.chase.1) {
                    let RegionDsp::Midi { start, length, cc, .. } = reg else { continue };
                    if pos < *start || pos >= *start + *length {
                        continue;
                    }
                    let before = cc.partition_point(|c| c.start < pos - *start);
                    for c in &cc[..before] {
                        last[c.kind.index()] = Some(c.value);
                    }
                }
                for (i, v) in last.iter().enumerate() {
                    if let Some(v) = v {
                        if t.midi.len() < t.midi.capacity() {
                            t.midi.push(MidiEvent { offset: 0, data: CcKind::from_index(i).message(*v) });
                            t.cc_sent = true;
                        }
                    }
                }
                // Chase: Notes. Notes held over the playhead start now (after the
                // controllers, so a sustain pedal is down first); the region loop below
                // ends them where they end.
                for reg in t.regions.iter().filter(|_| cx.chase.0) {
                    let RegionDsp::Midi { start, length, notes, .. } = reg else { continue };
                    for note in crate::edit::midi_content::sounding_at(notes, *start, *start + *length, pos) {
                        let p = note.pitch as usize & 127;
                        if !t.sounding[p] && t.midi.len() + 2 < t.midi.capacity() {
                            t.midi.push(MidiEvent { offset: 0, data: [0x90, note.pitch, note.velocity.max(1)] });
                            t.sounding[p] = true;
                        }
                    }
                }
            }
            if sources {
                for reg in t.regions.iter() {
                    let RegionDsp::Midi { start, length, notes, cc } = reg else { continue };
                    let (rs, re) = (*start, *start + *length);
                    if re <= pos || rs >= block_end {
                        continue;
                    }
                    let first = cc.partition_point(|c| rs + c.start < pos);
                    for c in &cc[first..] {
                        let at = rs + c.start;
                        if at >= block_end || at >= re || t.midi.len() >= t.midi.capacity() {
                            break;
                        }
                        t.midi.push(MidiEvent { offset: (at - pos) as u32, data: c.kind.message(c.value) });
                        t.cc_sent = true;
                    }
                    for note in notes.iter() {
                        let on = rs + note.start;
                        let off = (on + note.length).min(re);
                        if on >= block_end {
                            break;
                        }
                        if t.midi.len() + 2 > t.midi.capacity() {
                            break;
                        }
                        if on >= pos && on < re {
                            t.midi.push(MidiEvent { offset: (on - pos) as u32, data: [0x90, note.pitch, note.velocity.max(1)] });
                            t.sounding[note.pitch as usize & 127] = true;
                        }
                        if off >= pos && off < block_end && on < re {
                            t.midi.push(MidiEvent { offset: (off - pos) as u32, data: [0x80, note.pitch, 0] });
                            t.sounding[note.pitch as usize & 127] = false;
                        }
                    }
                }
            }
            if cx.midi_target == Some(t.id) {
                for m in cx.live {
                    if cx.midi_thru && t.midi.len() < t.midi.capacity() {
                        t.midi.push(*m);
                        track_off::note_held(&mut t.live_held, m.data);
                    }
                }
            }
            t.midi.sort_by_key(|m| m.offset);
            if let Some(ins) = &mut t.instrument {
                routes.enter(ins.uid);
                let ran = !ins.bypass && ll_skip & INSTRUMENT_BIT == 0;
                if ran {
                    ins.plugin.process(l, r, &t.midi, &transport);
                } else {
                    ins.plugin.pass_edits_aside(&transport);
                }
                if !routes.leave() {
                    return;
                }
                if ran {
                    check_output(cx.status, ins, l, r);
                    t.instrument_ran = true;
                }
            }
            // A frozen instrument track plays its freeze file instead.
            if sources {
                play_audio(&t.regions, pos, l, r, (0, 0));
            }
        }
    }

    track_off::fade_in(&mut t.resume_left, track_off::resume_frames(sr), l, r);

    for (k, slot) in t.inserts.iter_mut().enumerate().filter(|_| fx) {
        let Some(ins) = slot else { continue };
        let ran = !ins.bypass && ll_skip & (1 << k) == 0;
        // The plug-in reads its key from this thread's scratch: left in it (`Routes::leave`), it
        // must not read the source's own buffers, which the engine goes on writing, and frees with
        // the source's track.
        let key = match ran.then(|| key_signal(ins.key.idx.and_then(|i| routes.earlier(i)), ins.key.pre, iso, n)).flatten() {
            Some((a, b)) => {
                sc.key_l[..n].copy_from_slice(a);
                sc.key_r[..n].copy_from_slice(b);
                Some((&sc.key_l[..n], &sc.key_r[..n]))
            }
            None => None,
        };
        routes.enter(ins.uid);
        if ran {
            match &mut ins.plugin {
                Plugin::Builtin(b) => b.process(l, r, key, &fx_context(&transport)),
                Plugin::Vst(p) => p.process_keyed(l, r, key, &[], &transport),
                p => p.process(l, r, &[], &transport),
            }
        } else {
            ins.plugin.pass_edits_aside(&transport);
        }
        if !routes.leave() {
            return;
        }
        if ran {
            check_output(cx.status, ins, l, r);
        }
        if cx.analyzer_target == Some((t.id, ins.uid)) {
            routes.analyze(&mut t.outs, Some((&*l, &*r)), n);
        }
    }
    // The delay line keeps the post-plug-in signal; the output and the sends read it
    // at their own delays (none while live), gliding to a new delay (see `pdc.rs`).
    // It's written even while nothing reads it, so a glide always finds real audio.
    if let Some(d) = &mut t.delay {
        d.write(l, r);
    }
    let tap_delay = |d: u32| if live { 0 } else { d };
    let pdc_fade = (pdc::FADE_SECS * sr) as u32;

    let audible = !off
        && match role {
            Some(Role::Member { heard }) => heard,
            Some(Role::Key) => !p.mute,
            Some(_) => true,
            None => !p.mute && (!any_solo || p.solo),
        };
    // Pre-fader sends tap the signal after the plug-ins, before the fader and pan. Mute
    // and solo still silence them, as in Logic.
    for k in 0..SEND_SLOTS {
        match (p.sends[k], t.send_idx[k]) {
            // An isolated render leaves out auxes it doesn't play; their buses must
            // not fill up meanwhile, or they'd burst out when playback resumes.
            (Some(s), Some(j)) if j > ti && iso.map(|i| i.feeds(role, routes.id_of(j))).unwrap_or(true) => {
                if s.pre_fader {
                    let target = if audible { s.gain } else { 0.0 };
                    // As the fader's gains: a render starts at the level, however long the
                    // send had to glide there live.
                    if snap_gains {
                        t.send_gains[k] = target;
                    }
                    let tap = &mut t.pdc_taps.sends[k];
                    tap.aim(tap_delay(pdc.sends[k]), pdc_fade);
                    match &t.delay {
                        Some(d) if !tap.is_direct() => {
                            let (sl, sr) = (&mut sc.tap_l[..n], &mut sc.tap_r[..n]);
                            d.read_tap(tap, sl, sr);
                            routes.send(&mut t.outs, SendOut { slot: k, to: j, pre: true }, (&*sl, &*sr), &mut t.send_gains[k], target, smooth);
                        }
                        _ => routes.send(&mut t.outs, SendOut { slot: k, to: j, pre: true }, (&*l, &*r), &mut t.send_gains[k], target, smooth),
                    }
                }
            }
            _ => t.send_gains[k] = 0.0,
        }
    }
    let vol = if audible { p.volume } else { 0.0 };
    let mono_source = matches!(p.input, InputSel::Mono(_)) && t.kind == TrackKind::Audio;
    let (tl, tr) = if tap && !iso.map(|i| i.fader).unwrap_or(true) {
        (1.0, 1.0)
    } else if mono_source {
        // -3 dB compensated pan law: unity in the centre.
        let a = (p.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        (a.cos() * std::f32::consts::SQRT_2 * vol, a.sin() * std::f32::consts::SQRT_2 * vol)
    } else {
        // Balance for stereo sources.
        let pan = p.pan.clamp(-1.0, 1.0);
        ((1.0 - pan.max(0.0)) * vol, (1.0 + pan.min(0.0)) * vol)
    };
    let (mut gl, mut gr) = if snap_gains { (tl, tr) } else { t.gains };
    let (mut pl, mut pr) = (0.0f32, 0.0f32);
    // A side-chain key leaves as the signal leaves the plug-ins, before the
    // compensation delay, with the fader applied below.
    if t.keyed {
        t.key_l[..n].copy_from_slice(l);
        t.key_r[..n].copy_from_slice(r);
    }
    // A pre-fader key ignores the fader and pan but, like a pre-fader send, not mute.
    if t.keyed_pre {
        let g = if audible { 1.0 } else { 0.0 };
        for i in 0..n {
            t.pre_key_l[i] = l[i] * g;
            t.pre_key_r[i] = r[i] * g;
        }
    }
    t.pdc_taps.output.aim(tap_delay(pdc.output), pdc_fade);
    // Post-fader sends that read where the output reads share its result.
    let out_tap = t.pdc_taps.output;
    if let (false, Some(d)) = (out_tap.is_direct(), &t.delay) {
        d.read_tap(&mut t.pdc_taps.output, l, r);
    }
    let dest = match t.out_idx {
        // An isolated render captures the tap track, and anything feeding a stack
        // outside the render, straight off the bus.
        Some(j) if j > ti && !tap && iso.map(|i| i.feeds(role, routes.id_of(j))).unwrap_or(true) => Dest::Track(j),
        // A track that only keys a side-chain in an isolated render isn't heard.
        _ if role == Some(Role::Key) => Dest::Nowhere,
        _ => Dest::Bus,
    };
    for i in 0..n {
        gl += (tl - gl) * smooth;
        gr += (tr - gr) * smooth;
        sc.gain_l[i] = gl;
        sc.gain_r[i] = gr;
        l[i] *= gl;
        r[i] *= gr;
        pl = pl.max(l[i].abs());
        pr = pr.max(r[i].abs());
    }
    routes.output(&mut t.outs, dest, l, r);
    t.gains = (gl, gr);
    t.meter.feed(pl, pr);
    if t.keyed {
        for i in 0..n {
            t.key_l[i] *= sc.gain_l[i];
            t.key_r[i] *= sc.gain_r[i];
        }
    }
    // Post-fader sends take what the fader and pan left (already silent when muted).
    // One at another delay than the output reads its own tap and applies the same
    // fader gains.
    for k in 0..SEND_SLOTS {
        if let (Some(s), Some(j)) = (p.sends[k], t.send_idx[k]) {
            if !s.pre_fader && j > ti && iso.map(|i| i.feeds(role, routes.id_of(j))).unwrap_or(true) {
                if snap_gains {
                    t.send_gains[k] = s.gain;
                }
                let tap = &mut t.pdc_taps.sends[k];
                tap.aim(tap_delay(pdc.sends[k]), pdc_fade);
                match &t.delay {
                    Some(d) if *tap != out_tap => {
                        let (sl, sr) = (&mut sc.tap_l[..n], &mut sc.tap_r[..n]);
                        d.read_tap(tap, sl, sr);
                        for i in 0..n {
                            sl[i] *= sc.gain_l[i];
                            sr[i] *= sc.gain_r[i];
                        }
                        routes.send(&mut t.outs, SendOut { slot: k, to: j, pre: false }, (&*sl, &*sr), &mut t.send_gains[k], s.gain, smooth);
                    }
                    _ => {
                        t.pdc_taps.sends[k] = t.pdc_taps.output;
                        routes.send(&mut t.outs, SendOut { slot: k, to: j, pre: false }, (&*l, &*r), &mut t.send_gains[k], s.gain, smooth)
                    }
                }
            }
        }
    }
}
