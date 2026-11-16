//! Parameter values: CLAP's are in each parameter's own range, the engine's normalized 0..1.

use clap_sys::ext::params::*;
use clap_sys::plugin::clap_plugin;
use plugin_api::ParamInfo;

use crate::entry::buf_text;

/// A parameter's range, and what the host must know of its kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Range {
    pub min: f64,
    pub max: f64,
    /// It takes whole numbers only.
    pub stepped: bool,
    /// The plug-in reports it (a meter); the host doesn't set it.
    pub readonly: bool,
}

impl Range {
    fn of(info: &clap_param_info) -> Range {
        Range { min: info.min_value, max: info.max_value, stepped: info.flags & CLAP_PARAM_IS_STEPPED != 0, readonly: info.flags & CLAP_PARAM_IS_READONLY != 0 }
    }

    pub fn to_plain(self, normalized: f64) -> f64 {
        let v = self.min + normalized.clamp(0.0, 1.0) * (self.max - self.min);
        if self.stepped {
            v.round()
        } else {
            v
        }
    }

    pub fn to_normalized(self, plain: f64) -> f64 {
        if self.max > self.min {
            ((plain - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// VST3's step count: 0 for continuous, n for n+1 values.
    fn steps(self) -> i32 {
        if self.stepped {
            (self.max - self.min).round().max(1.0) as i32
        } else {
            0
        }
    }
}

/// Every parameter's range by id, sorted for lookups on the audio thread.
#[derive(Debug, Clone, Default)]
pub(crate) struct ParamRanges(Vec<(u32, Range)>);

impl ParamRanges {
    pub fn new(mut list: Vec<(u32, Range)>) -> ParamRanges {
        list.sort_by_key(|(id, _)| *id);
        ParamRanges(list)
    }

    /// Takes no lock and allocates nothing.
    pub fn get(&self, id: u32) -> Option<Range> {
        self.0.binary_search_by_key(&id, |(i, _)| *i).ok().map(|i| self.0[i].1)
    }
}

/// The plug-in's parameters as it describes them. Main thread.
pub(crate) unsafe fn infos(plugin: *const clap_plugin, params: *const clap_plugin_params) -> Vec<clap_param_info> {
    let Some(p) = params.as_ref() else { return Vec::new() };
    let (Some(count), Some(get_info)) = (p.count, p.get_info) else { return Vec::new() };
    let mut out = Vec::new();
    for i in 0..count(plugin) {
        let mut info: clap_param_info = std::mem::zeroed();
        if get_info(plugin, i, &mut info) {
            out.push(info);
        }
    }
    out
}

pub(crate) fn ranges(infos: &[clap_param_info]) -> ParamRanges {
    ParamRanges::new(infos.iter().map(|i| (i.id, Range::of(i))).collect())
}

/// The parameters automation and control surfaces may set.
pub(crate) fn automatable(infos: &[clap_param_info]) -> Vec<ParamInfo> {
    let hidden = CLAP_PARAM_IS_HIDDEN | CLAP_PARAM_IS_READONLY;
    infos
        .iter()
        .filter(|i| i.flags & CLAP_PARAM_IS_AUTOMATABLE != 0 && i.flags & hidden == 0)
        .map(|i| {
            let r = Range::of(i);
            ParamInfo { id: i.id, name: buf_text(&i.name), units: String::new(), default: r.to_normalized(i.default_value), steps: r.steps() }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_map_to_and_from_the_range() {
        let r = Range { min: -24.0, max: 12.0, stepped: false, readonly: false };
        assert_eq!(r.to_plain(0.5), -6.0);
        assert_eq!(r.to_normalized(-6.0), 0.5);
        assert_eq!(r.to_normalized(100.0), 1.0);
        assert_eq!(r.to_plain(-1.0), -24.0);
        let s = Range { min: 0.0, max: 3.0, stepped: true, readonly: false };
        assert_eq!(s.to_plain(0.4), 1.0);
        assert_eq!(s.steps(), 3);
        assert_eq!(Range { min: 1.0, max: 1.0, stepped: false, readonly: false }.to_normalized(1.0), 0.0);
    }

    #[test]
    fn ranges_are_found_by_id() {
        let r = |min| Range { min, max: 1.0, stepped: false, readonly: false };
        let rs = ParamRanges::new(vec![(9, r(0.5)), (2, r(0.0)), (5, r(0.25))]);
        assert_eq!(rs.get(5), Some(r(0.25)));
        assert_eq!(rs.get(2), Some(r(0.0)));
        assert_eq!(rs.get(3), None);
    }

    #[test]
    fn hidden_read_only_and_fixed_parameters_arent_automatable() {
        let info = |id, flags| {
            let mut i: clap_param_info = unsafe { std::mem::zeroed() };
            i.id = id;
            i.flags = flags;
            i.max_value = 2.0;
            i.default_value = 1.0;
            i
        };
        let all = [
            info(1, CLAP_PARAM_IS_AUTOMATABLE),
            info(2, CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_HIDDEN),
            info(3, CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_READONLY),
            info(4, 0),
            info(5, CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED),
        ];
        let got: Vec<(u32, f64, i32)> = automatable(&all).iter().map(|p| (p.id, p.default, p.steps)).collect();
        assert_eq!(got, vec![(1, 0.5, 0), (5, 0.5, 2)]);
    }
}
