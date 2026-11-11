#!/usr/bin/env bash
# Builds the engine's examples once and runs every headless check that needs no third-party
# plug-ins, on ALSA's `null` output device, the way CI runs them (.github/workflows/ci.yml).
#
# The checks run in an isolated environment: HOME and the XDG folders point into the scratch
# folder, and the audio-server variables are cleared, so they see no PipeWire, PulseAudio or
# JACK server and never read or write your settings, plug-in cache, presets or projects.
# The test tone comes from the `test_tone` example.
#
# usage: scripts/headless-checks.sh [--scratch DIR] [check ...]
#   --scratch DIR  where the projects, renders and logs go (default: a new temporary folder,
#                  removed afterwards when every check passed)
#   check ...      run only these checks (default: all of them)
# On Linux it needs the ALSA library and its `null` PCM (part of alsa-lib's standard config).
# Elsewhere (macOS) the engine stands in for that device with a silent thread of its own.
set -euo pipefail

# Each check and its arguments: "tone" is the test tone, "dir" its scratch folder.
CHECKS=(
  "stack_check tone dir"
  "loop_check tone dir"
  "sends_check tone dir"
  "send_auto_check tone dir"
  "bounce_check tone dir"
  "export_check tone dir"
  "range_edit_check tone dir"
  "automation_check tone dir"
  "insert_order_check tone dir"
  "fx_check tone dir"
  "fx_play_check tone dir"
  "takes_check tone dir"
  "take_tracks_check tone dir"
  "recovery_check tone dir"
  "crossfade_check tone dir"
  "dragmode_check tone dir"
  "midi_check dir"
  "countin_check dir"
  "tempo_check dir"
  "region_params_check tone dir"
  "strip_check tone dir"
  "controller_check tone dir"
  "feedback_check dir"
  "surface_check dir"
  "vca_check tone dir"
  "chase_check dir"
  "midi_replace_check dir"
  "midi_record_check dir"
  "projects_check tone dir"
  "arrangement_check dir"
  "autolanes_check tone dir"
  "pattern_check dir"
  "editing_check tone dir"
  "comping_check dir"
  "pdc_glide_check dir"
  "linphase_check dir"
  "arrange_cmds_check dir"
  "restore_check dir"
  "soak_check dir"
  "rt_alloc_check dir"
  "parallel_check dir"
  "instrument_check dir"
  "sampler_check dir"
  "drum_check dir"
  "onoff_check tone dir"
  "vst_sidechain_check tone dir"
  "multiout_check dir"
  "multiout_freeze_check dir"
  "vst_undo_check tone dir"
  "memlock_check dir"
)
# A check that hangs (a transport that never stops, say) fails instead of stalling CI.
TIMEOUT=${HEADLESS_CHECK_TIMEOUT:-600}

scratch=
only=()
while [ $# -gt 0 ]; do
  case $1 in
    --scratch) scratch=${2:?--scratch needs a folder}; shift 2 ;;
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) only+=("$1"); shift ;;
  esac
done

# (`${only[@]+...}`: macOS still ships bash 3.2, where an empty array is "unbound" under set -u.)
for name in ${only[@]+"${only[@]}"}; do
  case " ${CHECKS[*]%% *} " in
    *" $name "*) ;;
    *) echo "unknown check: $name" >&2; exit 2 ;;
  esac
done

made_scratch=
if [ -z "$scratch" ]; then
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/marbble-checks.XXXXXX")
  made_scratch=1
fi
mkdir -p "$scratch"
scratch=$(cd "$scratch" && pwd)

cd "$(dirname "$0")/.."

echo "== Building the engine examples"
cargo build -p daw-engine --examples
target=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
bin="$target/debug/examples"

# The environment of a fresh CI runner: an empty home, no session bus, no audio servers.
env_home="$scratch/_env"
rm -rf "$env_home"
mkdir -p "$env_home/.config" "$env_home/.local/share" "$env_home/.cache" "$env_home/.local/state" "$env_home/runtime"
chmod 700 "$env_home/runtime"
isolated=(
  env
  -u PIPEWIRE_RUNTIME_DIR -u PIPEWIRE_REMOTE -u PULSE_SERVER -u PULSE_RUNTIME_PATH
  -u JACK_DEFAULT_SERVER -u JACK_PROMISCUOUS_SERVER -u DBUS_SESSION_BUS_ADDRESS
  -u APPDATA -u USERPROFILE
  HOME="$env_home"
  XDG_CONFIG_HOME="$env_home/.config"
  XDG_DATA_HOME="$env_home/.local/share"
  XDG_CACHE_HOME="$env_home/.cache"
  XDG_STATE_HOME="$env_home/.local/state"
  XDG_RUNTIME_DIR="$env_home/runtime"
  JACK_NO_START_SERVER=1
)
# The time limit: coreutils' `timeout` (`gtimeout` from Homebrew's coreutils), or else, as on a
# stock Mac, the same in Perl, which macOS always has. Like `timeout`, it runs the check in a
# process group of its own, ends the whole group when time is up (TERM, then KILL 10 s later) and
# exits 124; otherwise it exits as the check did. It also kills whatever the check left running
# in its group, which would otherwise hold the pipe to `tee` open.
limit_perl='
use strict; use warnings; use POSIX ();
my $secs = shift;
my $pid = fork; die "fork: $!\n" unless defined $pid;
if (!$pid) { setpgrp(0, 0); exec { $ARGV[0] } @ARGV or POSIX::_exit(127) }
setpgrp($pid, $pid);   # in both processes, so the group exists before any kill below
my $timed_out = 0;
for my $sig (qw(INT TERM HUP)) { $SIG{$sig} = sub { kill $sig, -$pid } }
$SIG{ALRM} = sub {
  $timed_out = 1;
  kill "TERM", -$pid;
  $SIG{ALRM} = sub { kill "KILL", -$pid };
  alarm 10;
};
alarm $secs;
while (waitpid($pid, 0) < 0) { die "waitpid: $!\n" unless $!{EINTR} }
my $st = $?;
alarm 0;
kill "KILL", -$pid;
exit 124 if $timed_out;
exit(($st & 127) ? 128 + ($st & 127) : $st >> 8);
'
if command -v timeout > /dev/null; then
  isolated+=(timeout -k 10 "$TIMEOUT")
elif command -v gtimeout > /dev/null; then
  isolated+=(gtimeout -k 10 "$TIMEOUT")
else
  isolated+=(perl -e "$limit_perl" "$TIMEOUT")
fi

tone="$scratch/tone.wav"
"${isolated[@]}" "$bin/test_tone" "$tone"

failed=()
ran=0
for entry in "${CHECKS[@]}"; do
  read -r name args <<< "$entry"
  if [ ${#only[@]} -gt 0 ]; then
    case " ${only[*]} " in
      *" $name "*) ;;
      *) continue ;;
    esac
  fi
  dir="$scratch/$name"
  rm -rf "$dir"
  mkdir -p "$dir"
  argv=()
  for a in $args; do
    case $a in
      tone) argv+=("$tone") ;;
      dir) argv+=("$dir") ;;
    esac
  done
  echo
  echo "== $name"
  ran=$((ran + 1))
  status=0
  "${isolated[@]}" "$bin/$name" "${argv[@]}" 2>&1 | tee "$dir/output.log" || status=$?
  if [ "$status" -eq 124 ]; then
    echo "== $name timed out after ${TIMEOUT} s" | tee -a "$dir/output.log"
  fi
  # tempo_check reports failures only in its output, so look there too.
  if [ "$status" -ne 0 ] || grep -q '^FAIL' "$dir/output.log"; then
    failed+=("$name")
  fi
done

echo
if [ ${#failed[@]} -gt 0 ]; then
  echo "== ${#failed[@]} of $ran checks failed: ${failed[*]}"
  echo "   logs and projects: $scratch"
  exit 1
fi
echo "== All $ran checks passed"
if [ -n "$made_scratch" ]; then
  rm -rf "$scratch"
fi
