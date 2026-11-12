//! What the engine plays, built from the project: a track's parameters (`track_params`, VCAs
//! included), its regions as the engine plays them (`regions_dsp`: comps, patterns, region parameters
//! and the track's MIDI settings applied), its strip with plug-ins (`build_track`), and the whole graph (`rebuild_graph`).

use crate::session::*;

impl Session {
    /// What the engine plays for a track, with its stacks applied: a folder's fader scales its
    /// subtracks, a muted stack mutes them, and solo reaches both down (a soloed stack's
    /// subtracks) and up (the stacks a soloed subtrack plays through).
    pub(crate) fn track_params(p: &Project, t: &Track) -> TrackParams {
        let ancestors = p.ancestors(t.id);
        let mut volume_db = t.volume_db;
        let mut mute = t.mute;
        let mut solo = t.solo;
        let mut stack_db = 0.0;
        let mut stack_mute = false;
        let mut bus_mute = false;
        for a in ancestors.iter().filter_map(|id| p.track(*id)) {
            if a.kind == TrackKind::Folder {
                volume_db += a.volume_db;
                stack_db += a.volume_db;
            } else {
                bus_mute |= a.mute;
            }
            mute |= a.mute;
            stack_mute |= a.mute;
            solo |= a.solo;
        }
        // A VCA offsets the fader without moving it and mutes with it; like a folder's, both
        // still apply when automation drives the track's own volume and mute.
        let (vca_db, vca_mute) = crate::model::vca::offset(&p.vcas, t.vca.filter(|_| crate::model::vca::can_assign(t.kind)));
        volume_db += vca_db;
        stack_db += vca_db;
        mute |= vca_mute;
        stack_mute |= vca_mute;
        if t.kind.is_stack() {
            solo |= p.descendants(t.id).iter().filter_map(|id| p.track(*id)).any(|d| d.solo);
        }
        // Solo follows the routing too: a soloed track keeps the auxes it feeds audible, and a
        // soloed aux keeps the tracks feeding it audible. An aux on an instrument's extra output
        // doesn't need the instrument's strip audible, so that stays out of the solo.
        if !solo && p.tracks.iter().any(|y| y.solo) {
            let soloed = |y: &Track| y.solo || p.ancestors(y.id).iter().filter_map(|a| p.track(*a)).any(|a| a.solo);
            solo = p
                .tracks
                .iter()
                .filter(|y| y.id != t.id)
                .any(|y| (soloed(y) && p.reaches(y.id, t.id)) || (y.kind == TrackKind::Aux && y.solo && p.reaches_past_fader(t.id, y.id)));
        }
        // Summing stacks and auxes are buses; a folder passes its subtracks on to wherever it
        // plays. An explicit output overrides the stack.
        let output = p.output_of(t.id);
        let mut sends = [None; SEND_SLOTS];
        if t.kind != TrackKind::Folder {
            for (k, s) in t.sends.iter().take(SEND_SLOTS).enumerate() {
                if let Some(s) = s.filter(|s| s.bus != t.id && p.is_aux(s.bus)) {
                    let gain = if s.muted { 0.0 } else { volume_gain(s.level_db) };
                    sends[k] = Some(SendParams { bus: s.bus, gain, pre_fader: s.pre_fader });
                }
            }
        }
        TrackParams {
            volume: volume_gain(volume_db.max(-96.0)),
            pan: t.pan,
            mute,
            solo,
            armed: t.armed && t.kind.can_record(),
            monitor: t.monitor,
            input: t.input,
            output,
            sends,
            stack_gain: volume_gain(stack_db.max(-96.0)),
            stack_mute,
            folder: ancestors.iter().copied().find(|id| p.track(*id).map(|a| a.kind == TrackKind::Folder).unwrap_or(false)),
            own_gain: volume_gain(t.volume_db.max(-96.0)),
            own_mute: t.mute,
            bus_mute,
            vca_gain: volume_gain(vca_db.max(-96.0)),
            vca_mute,
            off: p.is_off(t.id),
            instrument_output: t.instrument_output,
            midi: t.midi_settings,
        }
    }

    /// Builds the playback list for a track. Audio regions don't stack: a newer region (later in
    /// the list) hides whatever older audio it covers, like Logic's "No Overlap" playback, but
    /// without destroying the older regions.
    ///
    /// A looping region plays as one entry per pass. Loop repetitions sit beneath every region
    /// body (they are laid down first), so a loop dragged over another region never hides it,
    /// as in Logic where the region covers the loop. The fade-in shapes the start of the first
    /// pass and the fade-out the end of the last one, at the loop end.
    ///
    /// Two regions that crossfade (see [`crate::edit::fades::crossfades`]) both play across the
    /// crossfade, the outgoing one past its end and the incoming one from before its start;
    /// outside it, the earlier one is heard before and the later one after, whichever is newer.
    pub(crate) fn regions_dsp(&self, t: &Track) -> Arc<Vec<RegionDsp>> {
        if let Some(f) = self.freeze_dsp(t).or_else(|| self.frozen_output_dsp(t)) {
            return f;
        }
        // Regions with playback parameters (see `session/regions/params.rs`).
        let map = self.params_map(t);
        let delayed = map.as_ref().and_then(|m| Self::delay_audio(t, m));
        let regions = delayed.as_deref().unwrap_or(&t.regions);
        // Pattern regions play the notes their patterns make (see `session/midi/patterns.rs`).
        let patterns = self.realize_patterns(regions);
        let regions = patterns.as_deref().unwrap_or(regions);
        let fades = AudioFades {
            // Settings › Editing › Crossfade Time and Curve, for comp boundaries and loop seams.
            xfade: self.crossfade_frames(),
            comp_law: FadeLaw::Curve(self.prefs.editing.crossfade_curve.clamp(-1.0, 1.0)),
            crossfades: crate::edit::fades::crossfades(regions, |c| self.clip_frames(c)),
        };
        let mut audio = AudioLayout::default();
        let mut out = Vec::new();
        // Loop repetitions first, then the region bodies, so bodies always win.
        for repeats in [true, false] {
            for r in regions.iter().filter(|r| !r.muted) {
                let passes = r.passes();
                let picked = if repeats { &passes[1..] } else { &passes[..1] };
                match &r.content {
                    RegionContent::Audio { .. } if r.takes.is_some() => self.lay_comp(r, &passes, picked, &fades, &mut audio),
                    RegionContent::Audio { .. } => self.lay_audio_region(r, &passes, picked, &fades, &mut audio),
                    RegionContent::Midi { .. } if !r.params.is_default() => {
                        out.extend(Self::midi_passes(r, picked, map.as_ref().unwrap()));
                    }
                    RegionContent::Midi { notes, cc } => {
                        if picked.is_empty() {
                            continue;
                        }
                        let mut n: Vec<Note> = notes.iter().filter(|n| !n.muted).copied().collect();
                        n.sort_by_key(|n| n.start);
                        let mut c = cc.clone();
                        c.sort_by_key(|c| c.start);
                        // Every pass shares one note list and one controller list.
                        let (n, c) = (Arc::new(n), Arc::new(c));
                        for &(ps, pl) in picked {
                            out.push(RegionDsp::Midi { start: ps, length: pl, notes: n.clone(), cc: c.clone() });
                        }
                    }
                }
            }
        }
        // The track's MIDI settings (see `session/tracks/midi_settings.rs`).
        self.play_through_track_midi(&t.midi_settings, &mut out);
        out.extend(audio.into_regions());
        Arc::new(out)
    }

    /// A clip's length in frames, if it's loaded.
    fn clip_frames(&self, clip: Id) -> Option<i64> {
        self.clips.get(&clip).map(|c| c.frames() as i64)
    }

    /// A take folder plays its comp: each pass in `picked` covers what's under it, then its pieces
    /// go in side by side so their crossfades overlap. The folder's own fades take its shape, the
    /// comp's crossfades the Crossfade Curve. Crossfades with neighbours replace the folder's own
    /// fades at those ends.
    fn lay_comp(&self, r: &Region, passes: &[(i64, i64)], picked: &[(i64, i64)], fades: &AudioFades, audio: &mut AudioLayout) {
        let (RegionContent::Audio { gain_db, fade_in, fade_out, fade_shape, .. }, Some(folder)) = (&r.content, &r.takes) else { return };
        let last = passes.last().map(|p| p.1).unwrap_or(r.length);
        let fi = (*fade_in).clamp(0, r.length);
        let fo = if r.is_looped() { (*fade_out).clamp(0, last) } else { (*fade_out).clamp(0, r.length - fi) };
        let x_in = fades.crossfades.iter().find(|x| x.in_region == r.id);
        let x_out = fades.crossfades.iter().find(|x| x.out_region == r.id);
        let own_in = x_in.map(|x| (x.fade_in, x.laws().1)).unwrap_or(((r.start, fi), FadeLaw::fade_in(fade_shape)));
        let own_out = x_out.map(|x| (x.fade_out, x.laws().0)).unwrap_or(((r.end() - fo, fo), FadeLaw::fade_out(fade_shape)));
        let keep: Vec<Id> = x_in.map(|x| x.out_region).into_iter().chain(x_out.map(|x| x.in_region)).collect();
        for &(ps, pl) in picked {
            let mut edges = crate::edit::takes::pieces::PassEdges::default();
            if let (Some(x), true) = (x_in, ps == r.start) {
                (edges.lead, edges.fade_in) = (ps - x.fade_in.0, Some(x.fade_in));
            }
            if let (Some(x), true) = (x_out, ps + pl >= r.end()) {
                (edges.tail, edges.fade_out) = (x.fade_out.0 + x.fade_out.1 - (ps + pl), Some(x.fade_out));
            }
            audio.cut(ps - edges.lead.max(0), ps + pl + edges.tail.max(0), &keep);
            for p in crate::edit::takes::pieces::pass_pieces_edged(r, folder, ps, pl, fades.xfade, edges) {
                let Some(c) = self.clips.get(&p.clip) else { continue };
                let gain = volume_gain(*gain_db + p.gain_db);
                let law_in = if own_in.0 .1 > 0 && p.fade_in == own_in.0 { own_in.1.clone() } else { fades.comp_law.clone() };
                let law_out = if own_out.0 .1 > 0 && p.fade_out == own_out.0 { own_out.1.clone() } else { fades.comp_law.clone() };
                audio.segs.push(Seg {
                    region: r.id,
                    start: p.start,
                    end: p.end,
                    offset: p.offset,
                    gain,
                    clip: c.clone(),
                    fade_in: p.fade_in,
                    fade_out: p.fade_out,
                    laws: [law_in, law_out],
                });
            }
        }
    }

    /// An audio region's passes in `picked`. The first pass fades in and the last fades out, with
    /// the region's own fades or a crossfade, which stretches the region to cover it; repetitions
    /// that crossfade overlap each other at the seams.
    fn lay_audio_region(&self, r: &Region, passes: &[(i64, i64)], picked: &[(i64, i64)], fades: &AudioFades, audio: &mut AudioLayout) {
        let RegionContent::Audio { clip, offset, gain_db, fade_in, fade_out, fade_shape } = &r.content else { return };
        let Some(c) = self.clips.get(clip) else { return };
        let (end, last) = (r.end(), passes.last().map(|p| p.1).unwrap_or(r.length));
        let fi = (*fade_in).clamp(0, r.length);
        let fo = if r.is_looped() { (*fade_out).clamp(0, last) } else { (*fade_out).clamp(0, r.length - fi) };
        let mut first_in = ((r.start, fi), FadeLaw::fade_in(fade_shape));
        let mut last_out = ((end - fo, fo), FadeLaw::fade_out(fade_shape));
        let mut keep = Vec::new();
        if let Some(x) = fades.crossfades.iter().find(|x| x.in_region == r.id) {
            first_in = (x.fade_in, x.laws().1);
            keep.push(x.out_region);
        }
        if let Some(x) = fades.crossfades.iter().find(|x| x.out_region == r.id) {
            last_out = (x.fade_out, x.laws().0);
            keep.push(x.in_region);
        }
        let seams = crate::edit::fades::loop_seams(r, |c| self.clip_frames(c), fades.xfade);
        if !seams.is_empty() {
            keep.push(r.id);
        }
        let seam_law = FadeLaw::Curve(crate::edit::fades::DEFAULT_CURVE);
        let (first, n) = (r.start, passes.len());
        for &(ps, pl) in picked {
            let k = passes.iter().position(|p| p.0 == ps).unwrap_or(0);
            let (fin, law_in) = match (ps == first, k.checked_sub(1).and_then(|j| seams.get(j))) {
                (true, _) => first_in.clone(),
                (false, Some(z)) => (z.1, seam_law.clone()),
                (false, None) => ((ps, 0), FadeLaw::default()),
            };
            let (fout, law_out) = match (k + 1 == n, seams.get(k)) {
                (true, _) => last_out.clone(),
                (false, Some(z)) => (z.0, seam_law.clone()),
                (false, None) => ((ps + pl, 0), FadeLaw::default()),
            };
            let (s, e) = (ps.min(fin.0), (ps + pl).max(fout.0 + fout.1));
            let seg = Seg {
                region: r.id,
                start: s,
                end: e,
                offset: *offset + (s - ps),
                gain: volume_gain(*gain_db),
                clip: c.clone(),
                fade_in: fin,
                fade_out: fout,
                laws: [law_in, law_out],
            };
            audio.lay(seg, &keep);
        }
    }

    pub(crate) fn instantiate_slot(&mut self, slot: &PluginSlot) -> Result<InsertDsp> {
        let sr = self.sample_rate();
        // Undo frees instance ids for reuse: one that held a built-in effect may come back as
        // another kind of plug-in, which mustn't inherit its latency or meters.
        if !crate::session::plugins::builtin::is_builtin(&slot.plugin_id) {
            self.fx.remove(&slot.uid);
        }
        let plugin = if let Some(info) = crate::fx::find(&slot.plugin_id) {
            self.instantiate_builtin(&info, slot)
        } else if let Some(info) = crate::instrument::find(&slot.plugin_id) {
            self.instantiate_instrument(&info, slot)
        } else {
            let desc = self.find_plugin(&slot.plugin_id).ok_or_else(|| anyhow!("plug-in not found: {}", slot.name))?.clone();
            let (proc, ctrl) = vst3_host::instantiate_keyed(&desc, sr, MAX_BLOCK, slot.sidechain.is_some()).map_err(|e| anyhow!(e))?;
            if let Some(st) = &slot.state {
                ctrl.load_state(&decode_state(st));
            }
            self.track_state_copy(slot, &ctrl);
            self.controllers.insert(slot.uid, ctrl);
            Plugin::Vst(Box::new(proc))
        };
        Ok(InsertDsp::new(slot.uid, slot.bypass, plugin, KeyRoute::new(slot.sidechain, slot.sidechain_pre)))
    }

    pub(crate) fn build_track(&mut self, t: &Track) -> Box<TrackDsp> {
        let meter = self.meters.entry(t.id).or_default().clone();
        let mut dsp = TrackDsp::new(t.id, t.kind, Self::track_params(&self.project, t), meter);
        dsp.load = self.strip_loads.entry(t.id).or_default().clone();
        if self.ahead.is_some() {
            dsp.prepare_ahead();
        }
        self.load_clips(t.clips());
        dsp.set_regions(self.regions_dsp(t));
        dsp.set_automation(lanes_dsp(&t.automation), t.auto_mode);
        // A new engine track starts without delay compensation; update_pdc sends it.
        self.pdc_sent.remove(&t.id);
        if t.frozen.is_some() {
            // A frozen track plays its freeze file; its plug-ins stay unloaded.
            return Box::new(dsp);
        }
        if let Some(slot) = &t.instrument {
            match self.instantiate_slot(slot) {
                Ok(i) => dsp.set_instrument(Some(i)),
                Err(e) => log::error!("{}: {e}", t.name),
            }
        }
        for (i, slot) in t.inserts.iter().enumerate() {
            if let Some(slot) = slot {
                match self.instantiate_slot(slot) {
                    Ok(ins) => dsp.set_insert(i, Some(ins)),
                    Err(e) => log::error!("{}: {e}", t.name),
                }
            }
        }
        Box::new(dsp)
    }

    /// Recreates every track and plugin from the project document.
    pub(crate) fn rebuild_graph(&mut self) {
        // Every command dropped is made good by this: those still queued are applied to the old
        // graph before the swap (`Engine::replace_graph`), so none undoes part of the new one.
        self.engine_resync = false;
        // The plug-ins threads are still left in keep their editors hidden, untold.
        self.mark_unresponsive();
        let snapshot = self.capture_plugin_states();
        self.adopt_states(&snapshot, true);
        for c in self.controllers.values_mut() {
            c.close_editor();
        }
        let old_controllers = std::mem::take(&mut self.controllers);
        self.fx.clear();
        // The task graph goes in with the strips, so that their first block can already use the
        // pool: sent after the swap (`sync_parallel`), it would come a block or more late, and a
        // heavy project's block played serially overloads (#468). Planned before the plug-ins
        // load, so that any workers it starts are up by then.
        self.graph_dirty = false;
        let graph = self.plan_parallel();
        self.threads.graph.clone_from(&graph);
        let tracks: Vec<Track> = snapshot.tracks.clone();
        let mut built: Vec<Box<TrackDsp>> = tracks.iter().map(|t| self.build_track(t)).collect();
        // The engine plays tracks in this order; routes only point forwards.
        let order = snapshot.process_order();
        built.sort_by_key(|t| order.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
        let mut master = Vec::new();
        for slot in &snapshot.master.inserts {
            master.push(slot.as_ref().and_then(|s| self.instantiate_slot(s).map_err(|e| log::error!("{e}")).ok()));
        }
        let sr = self.project.sample_rate as f64;
        // The one lock outside an offline render: a swap, with the old graph dropped after.
        let old = self.engine.lock().replace_graph(sr, built, master, graph.map(Box::new));
        self.engine_rate = sr;
        drop(old);
        drop(old_controllers);
        // Another project (or the same one reloaded): its strips start without clips.
        self.peak_holds.reset(None);
        self.sync_master_automation();
        self.sync_globals();
    }
}

/// The fades `regions_dsp` lays a track's audio with.
struct AudioFades {
    /// Crossfade Time, in frames.
    xfade: i64,
    /// The Crossfade Curve's law, for the seams between a comp's pieces.
    comp_law: FadeLaw,
    /// The crossfades between the track's regions.
    crossfades: Vec<crate::edit::fades::Crossfade>,
}

/// A stretch of one region's audio, as the engine will play it.
struct Seg {
    region: Id,
    start: i64,
    end: i64,
    offset: i64,
    gain: f32,
    clip: Arc<Clip>,
    fade_in: (i64, i64),
    fade_out: (i64, i64),
    laws: [FadeLaw; 2],
}

/// A track's audio as it is laid down, newer pieces over older ones.
#[derive(Default)]
struct AudioLayout {
    segs: Vec<Seg>,
}

impl AudioLayout {
    /// Cuts away the older audio under `rs..re`, except a crossfade partner's (`keep`).
    fn cut(&mut self, rs: i64, re: i64, keep: &[Id]) {
        let mut kept = Vec::with_capacity(self.segs.len() + 1);
        for sg in self.segs.drain(..) {
            if sg.end <= rs || sg.start >= re || keep.contains(&sg.region) {
                kept.push(sg);
                continue;
            }
            if sg.start < rs {
                kept.push(Seg { start: sg.start, end: rs, clip: sg.clip.clone(), laws: sg.laws.clone(), ..sg });
            }
            if sg.end > re {
                kept.push(Seg { start: re, offset: sg.offset + (re - sg.start), ..sg });
            }
        }
        self.segs = kept;
    }

    /// Lays a new piece of audio over what's there.
    fn lay(&mut self, new: Seg, keep: &[Id]) {
        self.cut(new.start, new.end, keep);
        self.segs.push(new);
    }

    /// The pieces as the engine plays them, in time order.
    fn into_regions(mut self) -> impl Iterator<Item = RegionDsp> {
        self.segs.sort_by_key(|s| s.start);
        self.segs.into_iter().map(|s| RegionDsp::Audio {
            start: s.start,
            length: s.end - s.start,
            offset: s.offset,
            gain: s.gain,
            clip: s.clip,
            fade_in: s.fade_in,
            fade_out: s.fade_out,
            laws: s.laws,
        })
    }
}
