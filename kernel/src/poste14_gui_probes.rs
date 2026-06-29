// Extracted from main.rs to keep the kernel entry file maintainable.
// These probes are readiness scaffolding over established runtime state, not
// independent proof of each named GUI property.
use crate::{arch, scheduler, serial, subsystem_validation};

mod cycle_five;
mod cycle_four;
mod cycle_three;
mod cycle_two;
mod runtime;
mod v3_chain;

use cycle_five::{
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended5,
    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended5,
};
use cycle_four::{
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline_extended4,
    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline_extended4,
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended4,
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline_extended4,
    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended4,
};
use cycle_three::{
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline_extended3,
    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline_extended3,
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended3,
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline_extended3,
    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended3,
};
use cycle_two::{
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline_extended2,
    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline_extended2,
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended2,
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline_extended2,
    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended2,
};
use v3_chain::{
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline,
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline_extended,
    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline,
    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline_extended,
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline,
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended,
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline,
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline_extended,
    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline,
    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended,
};

pub(crate) use runtime::{
    probe_poste14_gui_app_lifecycle_baseline, probe_poste14_gui_event_ordering_baseline,
    probe_poste14_gui_focus_arbitration_baseline, probe_poste14_gui_focus_recovery_baseline,
    probe_poste14_gui_input_routing_baseline, probe_poste14_gui_recovery_escalation_baseline,
    probe_poste14_gui_runtime_composition_baseline, probe_poste14_gui_runtime_ownership_baseline,
    probe_subsystem_state_refactored,
};

fn subsystem_health_triplet() -> (bool, bool, bool) {
    let scheduler_ok = subsystem_validation::validate_scheduler_operational();
    let process_ok = subsystem_validation::validate_process_subsystem_present();
    let syscall_ok = subsystem_validation::validate_syscall_dispatch_safe();
    (scheduler_ok, process_ok, syscall_ok)
}

pub(super) fn run_poste14_gui_probe_chain() {
    serial::write_line("gui-probe-chain: readiness scaffolding (not independent behavioral proof)");

    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline();
    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline();
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline();
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline();
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline();

    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended();
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended();
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline_extended();
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline_extended();
    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline_extended();

    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended2();
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended2();
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline_extended2();
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline_extended2();
    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline_extended2();

    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended3();
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended3();
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline_extended3();
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline_extended3();
    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline_extended3();

    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended4();
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended4();
    probe_poste14_gui_continuity_hysteresis_envelope_recovery_v3_baseline_extended4();
    probe_poste14_gui_hysteresis_envelope_recovery_guardrails_v3_baseline_extended4();
    probe_poste14_gui_envelope_recovery_guardrails_continuity_v3_baseline_extended4();
    probe_poste14_gui_recovery_guardrails_continuity_hysteresis_v3_baseline_extended5();
    probe_poste14_gui_guardrails_continuity_hysteresis_envelope_v3_baseline_extended5();
}
