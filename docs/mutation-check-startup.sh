#!/usr/bin/env bash
# Canary check for the startup wrappers: the background whisper fetch and the
# state machine's run guard. Mutate one decision at a time and record which tests
# notice. A mutation that leaves the suite green is a hole in the tests.
#
# Run from v-2/voice-ptt:  bash ../docs/mutation-check-startup.sh
set -u

# shellcheck source=docs/canary-harness.sh
source "$(dirname "$0")/canary-harness.sh"

M=src/asr/model_load.rs
R=src/state/machine_run.rs

canary_init "$M" "$R"

echo "=== canaries: mutate one decision, expect red ==="

# ---- the background model fetch ---------------------------------------------

mutate "$M" \
  '        if engine_ready {
            return ModelLoad::NotNeeded;
        }' \
  '' \
  'S1 a model that is already loaded is downloaded again'

mutate "$M" \
  '            Ok(path) if reloaded => ModelLoad::Ready { path },' \
  '            Ok(path) if true => ModelLoad::Ready { path },' \
  'S2 a refused model is reported as ready'

mutate "$M" \
  '            Ok(path) => ModelLoad::ReloadRefused { path },' \
  '            Ok(path) => ModelLoad::Ready { path },' \
  'S3 a refused model is reported as ready (second route)'

mutate "$M" \
  '            ModelLoad::ReloadRefused { .. } => Severity::Warn,' \
  '            ModelLoad::ReloadRefused { .. } => Severity::Info,' \
  'S4 a refused model is logged as a footnote'

mutate "$M" \
  '            ModelLoad::DownloadFailed { .. } => Severity::Error,' \
  '            ModelLoad::DownloadFailed { .. } => Severity::Warn,' \
  'S5 a failed download is logged as a warning'

mutate "$M" \
  '        matches!(self, ModelLoad::NotNeeded | ModelLoad::Ready { .. })' \
  '        matches!(self, ModelLoad::NotNeeded | ModelLoad::Ready { .. } | ModelLoad::ReloadRefused { .. })' \
  'S6 a model the engine refused counts as usable'

# ---- the state machine run guard ---------------------------------------------

mutate "$R" \
  '    match tokio::task::spawn(AssertUnwindSafe(fut)).await {' \
  '    match Ok::<Result<(), E>, std::convert::Infallible>(fut.await) {' \
  'S7 the panic guard is removed and a panic unwinds into the caller'

mutate "$R" \
  '        Err(join_err) => MachineExit::Panicked(join_err.to_string()),' \
  '        Err(_) => MachineExit::Clean,' \
  'S8 a panicking machine is reported as a clean exit'

mutate "$R" \
  '        Err(join_err) => MachineExit::Panicked(join_err.to_string()),' \
  '        Err(_) => MachineExit::Panicked("".into()),' \
  'S9 the panic cause is dropped from the report'

mutate "$R" \
  '        if self.machine_is_live() {
            Severity::Info
        } else {
            Severity::Error
        }' \
  '        Severity::Info' \
  'S10 a dead machine is reported as an ordinary exit'

mutate "$R" \
  '        matches!(self, MachineExit::Clean)' \
  '        !matches!(self, MachineExit::Clean)' \
  'S11 a dead machine is reported as still live'

mutate "$R" \
  '            MachineExit::Panicked(e) => format!(
                "state machine task PANICKED — hotkeys/transcription are down until restart: {e}"
            ),' \
  '            MachineExit::Panicked(e) => format!("task panicked: {e}"),' \
  'S12 the panic report stops saying what is broken'

canary_finish