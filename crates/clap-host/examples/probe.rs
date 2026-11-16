//! Scans installed CLAP plug-ins, instantiates each one and runs a few blocks through it, as
//! `vst3-host`'s probe does for VST3s: `cargo run -p clap-host --example probe -- [file] [name]`.
use clap_host::instantiate;
use clap_host::scan::{default_search_paths, find_files, scan_file};
use plugin_api::{Controller, MidiEvent, Processor, TransportInfo};

fn main() {
    let filter = std::env::args().nth(1);
    let only = std::env::args().nth(2);
    for f in find_files(&default_search_paths()) {
        if filter.as_ref().is_some_and(|x| !f.to_string_lossy().contains(x.as_str())) {
            continue;
        }
        let descs = match scan_file(&f) {
            Ok(d) => d,
            Err(e) => {
                println!("SCAN FAIL {}: {e}", f.display());
                continue;
            }
        };
        println!("{}: {} plug-ins", f.display(), descs.len());
        let shown: Vec<_> = match &only {
            Some(o) => descs.iter().filter(|d| d.name.contains(o.as_str())).collect(),
            None => descs.iter().take(3).collect(),
        };
        for d in shown {
            let (mut p, mut c) = match instantiate(d, 48000.0, 512, false) {
                Ok(x) => x,
                Err(e) => {
                    println!("  FAIL {} : {e}", d.name);
                    continue;
                }
            };
            let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
            let t = TransportInfo { sample_rate: 48000.0, tempo: 120.0, sig_num: 4, sig_den: 4, ..Default::default() };
            let mut peak = 0.0f32;
            for i in 0..20 {
                for (j, s) in l.iter_mut().enumerate() {
                    *s = ((i * 512 + j) as f32 * 0.05).sin() * 0.5;
                }
                r.copy_from_slice(&l);
                let midi = [MidiEvent { offset: 0, data: [0x90, 60, 100] }];
                p.process(&mut l, &mut r, if i == 0 { &midi } else { &[] }, &t);
                peak = peak.max(l.iter().fold(0.0f32, |a, &b| a.max(b.abs())));
                c.idle();
            }
            for bus in c.audio_buses() {
                println!(
                    "         {} port \"{}\": {} ch{}",
                    if bus.input { "in " } else { "out" },
                    bus.name,
                    bus.channels,
                    if bus.aux { ", aux" } else { ", main" }
                );
            }
            let st = c.save_state().unwrap_or_default();
            println!(
                "  OK   {:<28} [{}] id={} vendor={} inst={} params={} latency={} state={}B peak={peak:.3}",
                d.name,
                d.subcategories,
                d.cid,
                d.vendor,
                d.is_instrument,
                c.parameters().len(),
                c.latency(),
                st.component.len()
            );
        }
    }
}
