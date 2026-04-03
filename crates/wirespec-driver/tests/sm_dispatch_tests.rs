// crates/wirespec-driver/tests/sm_dispatch_tests.rs
//
// Runtime tests for generated state machine C code. Generates C code from
// wirespec SM definitions, compiles with a test driver, executes the binary,
// and verifies state transitions produce correct results.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use wirespec_backend_api::*;
use wirespec_sema::ComplianceProfile;
use wirespec_syntax::parse;

fn generate_c(src: &str, prefix: &str) -> (String, String) {
    let ast = parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = wirespec_backend_c::CBackend;
    let ctx = BackendContext {
        module_name: prefix.into(),
        module_prefix: prefix.into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_c::checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();
    (
        lowered.header_content.clone(),
        lowered.source_content.clone(),
    )
}

/// Compile a C test driver together with generated SM code and run it.
/// Returns (success, stdout, stderr).
fn compile_and_run(
    header: &str,
    source: &str,
    driver: &str,
    prefix: &str,
) -> (bool, String, String) {
    let dir = PathBuf::from(format!("/tmp/wirespec-sm-dispatch-{prefix}"));
    // Clean the directory to avoid stale files from prior runs
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let runtime_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runtime");

    std::fs::write(dir.join(format!("{prefix}.h")), header).unwrap();
    std::fs::write(dir.join(format!("{prefix}.c")), source).unwrap();
    std::fs::write(dir.join(format!("{prefix}_driver.c")), driver).unwrap();

    let binary = dir.join(format!("{prefix}_runner"));

    // Compile
    let compile = match Command::new("gcc")
        .args([
            "-Wall",
            "-Wextra",
            "-Werror",
            "-std=c11",
            "-I",
            &dir.to_string_lossy(),
            "-I",
            &runtime_dir.to_string_lossy(),
        ])
        .arg(dir.join(format!("{prefix}.c")))
        .arg(dir.join(format!("{prefix}_driver.c")))
        .arg("-o")
        .arg(&binary)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return (false, String::new(), format!("gcc not found: {e}"));
        }
    };

    if !compile.status.success() {
        let stderr = String::from_utf8_lossy(&compile.stderr).to_string();
        return (
            false,
            String::new(),
            format!("compilation failed:\n{stderr}"),
        );
    }

    // Run
    let run = match Command::new(&binary).output() {
        Ok(o) => o,
        Err(e) => {
            return (false, String::new(), format!("failed to execute: {e}"));
        }
    };

    let stdout = String::from_utf8_lossy(&run.stdout).to_string();
    let stderr = String::from_utf8_lossy(&run.stderr).to_string();
    (run.status.success(), stdout, stderr)
}

#[test]
fn sm_dispatch_basic_transitions() {
    // SM with explicit action on start to ensure count is initialized.
    // Tests: init, basic transition, self-transition with guard+action,
    // guard rejection, terminal state, wildcard transitions.
    let sm_source = r#"
        state machine TestSm {
            state Idle
            state Running { count: u8 = 0 }
            state Done [terminal]
            initial Idle

            transition Idle -> Running {
                on start
                action { dst.count = 0; }
            }
            transition Running -> Running {
                on tick
                guard src.count < 3
                action { dst.count = src.count + 1; }
            }
            transition Running -> Done { on finish }
            transition * -> Done { on abort }
        }
    "#;

    let (header, source) = generate_c(sm_source, "tsm");

    // Print generated code for debugging
    eprintln!("=== Generated Header ===\n{header}");
    eprintln!("=== Generated Source ===\n{source}");

    let driver = r#"
#include <stdio.h>
#include <string.h>
#include "tsm.h"

int main(void) {
    tsm_test_sm_t sm;
    wirespec_result_t r;
    int pass = 0, fail = 0;

    /* Test 1: Initial state is Idle */
    memset(&sm, 0, sizeof(sm));
    tsm_test_sm_init(&sm);
    if (sm.tag == TSM_TEST_SM_IDLE) { pass++; printf("PASS: initial state is Idle\n"); }
    else { fail++; printf("FAIL: initial state, got tag=%d\n", sm.tag); }

    /* Test 2: Idle -> start -> Running (with count=0 from action) */
    memset(&sm, 0, sizeof(sm));
    tsm_test_sm_init(&sm);
    r = tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_START });
    if (r == WIRESPEC_OK && sm.tag == TSM_TEST_SM_RUNNING && sm.running.count == 0) {
        pass++; printf("PASS: Idle -> start -> Running (count=0)\n");
    } else { fail++; printf("FAIL: Idle -> start, r=%d tag=%d count=%d\n", r, sm.tag, sm.running.count); }

    /* Test 3: Running -> tick -> Running with count incremented */
    r = tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_TICK });
    if (r == WIRESPEC_OK && sm.tag == TSM_TEST_SM_RUNNING && sm.running.count == 1) {
        pass++; printf("PASS: tick -> count=1\n");
    } else { fail++; printf("FAIL: tick, r=%d tag=%d count=%d\n", r, sm.tag, sm.running.count); }

    /* Test 4: Tick twice more, count should reach 3 */
    tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_TICK }); /* count=2 */
    tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_TICK }); /* count=3 */
    if (sm.tag == TSM_TEST_SM_RUNNING && sm.running.count == 3) {
        pass++; printf("PASS: tick x3 -> count=3\n");
    } else { fail++; printf("FAIL: tick x3, tag=%d count=%d\n", sm.tag, sm.running.count); }

    /* Test 5: Guard rejects tick when count >= 3 */
    r = tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_TICK });
    if (r != WIRESPEC_OK && sm.tag == TSM_TEST_SM_RUNNING) {
        pass++; printf("PASS: guard rejects tick at count=3\n");
    } else { fail++; printf("FAIL: guard should reject, r=%d tag=%d\n", r, sm.tag); }

    /* Test 6: Running -> finish -> Done */
    r = tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_FINISH });
    if (r == WIRESPEC_OK && sm.tag == TSM_TEST_SM_DONE) {
        pass++; printf("PASS: Running -> finish -> Done\n");
    } else { fail++; printf("FAIL: finish, r=%d tag=%d\n", r, sm.tag); }

    /* Test 7: Terminal state rejects all events */
    r = tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_TICK });
    if (r == WIRESPEC_ERR_INVALID_STATE) {
        pass++; printf("PASS: terminal rejects tick\n");
    } else { fail++; printf("FAIL: terminal should reject tick, r=%d\n", r); }

    r = tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_ABORT });
    if (r != WIRESPEC_OK) {
        pass++; printf("PASS: terminal rejects abort\n");
    } else { fail++; printf("FAIL: terminal should reject abort, r=%d\n", r); }

    /* Test 8: Wildcard abort from Idle */
    memset(&sm, 0, sizeof(sm));
    tsm_test_sm_init(&sm);
    r = tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_ABORT });
    if (r == WIRESPEC_OK && sm.tag == TSM_TEST_SM_DONE) {
        pass++; printf("PASS: Idle -> abort -> Done\n");
    } else { fail++; printf("FAIL: abort from Idle, r=%d tag=%d\n", r, sm.tag); }

    /* Test 9: Wildcard abort from Running */
    memset(&sm, 0, sizeof(sm));
    tsm_test_sm_init(&sm);
    tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_START });
    r = tsm_test_sm_dispatch(&sm, &(tsm_test_sm_event_t){ .tag = TSM_TEST_SM_EVENT_ABORT });
    if (r == WIRESPEC_OK && sm.tag == TSM_TEST_SM_DONE) {
        pass++; printf("PASS: Running -> abort -> Done\n");
    } else { fail++; printf("FAIL: abort from Running, r=%d tag=%d\n", r, sm.tag); }

    printf("\n%d passed, %d failed\n", pass, fail);
    return fail > 0 ? 1 : 0;
}
"#;

    let (ok, stdout, stderr) = compile_and_run(&header, &source, driver, "tsm");
    eprintln!("=== stdout ===\n{stdout}");
    if !stderr.is_empty() {
        eprintln!("=== stderr ===\n{stderr}");
    }
    assert!(
        ok,
        "SM dispatch test failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("0 failed"),
        "Some SM dispatch checks failed:\n{stdout}"
    );
}

#[test]
fn sm_dispatch_delegate() {
    let sm_source = r#"
        state machine ChildSm {
            state Ready
            state Done [terminal]
            initial Ready

            transition Ready -> Done { on finish }
            transition * -> Done { on force_close }
        }

        state machine ParentSm {
            state Running { child: ChildSm, counter: u8 = 0 }
            state Complete [terminal]
            initial Running

            transition Running -> Running {
                on forward_to_child(child_ev_tag: u8)
                delegate src.child <- child_ev_tag
            }
            transition Running -> Complete {
                on child_state_changed
                guard src.child in_state(Done)
            }
            transition * -> Complete { on shutdown }
        }
    "#;

    let (header, source) = generate_c(sm_source, "del");

    eprintln!("=== Delegate Header ===\n{header}");
    eprintln!("=== Delegate Source ===\n{source}");

    let driver = r#"
#include <stdio.h>
#include <string.h>
#include "del.h"

int main(void) {
    del_parent_sm_t sm;
    wirespec_result_t r;
    int pass = 0, fail = 0;

    /* Test 1: Init parent -> Running, child -> Ready */
    memset(&sm, 0, sizeof(sm));
    del_parent_sm_init(&sm);
    del_child_sm_init(&sm.running.child);
    if (sm.tag == DEL_PARENT_SM_RUNNING && sm.running.child.tag == DEL_CHILD_SM_READY) {
        pass++; printf("PASS: parent=Running, child=Ready\n");
    } else { fail++; printf("FAIL: init, ptag=%d ctag=%d\n", sm.tag, sm.running.child.tag); }

    /* Test 2: Delegate finish event to child -> child Done, parent auto-transitions to Complete */
    memset(&sm, 0, sizeof(sm));
    del_parent_sm_init(&sm);
    del_child_sm_init(&sm.running.child);
    r = del_parent_sm_dispatch(&sm,
        &(del_parent_sm_event_t){
            .tag = DEL_PARENT_SM_EVENT_FORWARD_TO_CHILD,
            .forward_to_child = { .child_ev_tag = DEL_CHILD_SM_EVENT_FINISH }
        });
    /* The delegate dispatches finish to child (Ready->Done), then since child
       state changed, it auto-fires child_state_changed, and the guard
       (child in_state Done) passes, moving parent to Complete. */
    if (r == WIRESPEC_OK && sm.tag == DEL_PARENT_SM_COMPLETE) {
        pass++; printf("PASS: delegate finish -> parent=Complete\n");
    } else { fail++; printf("FAIL: delegate, r=%d ptag=%d\n", r, sm.tag); }

    /* Test 3: Wildcard shutdown from Running */
    memset(&sm, 0, sizeof(sm));
    del_parent_sm_init(&sm);
    del_child_sm_init(&sm.running.child);
    r = del_parent_sm_dispatch(&sm,
        &(del_parent_sm_event_t){ .tag = DEL_PARENT_SM_EVENT_SHUTDOWN });
    if (r == WIRESPEC_OK && sm.tag == DEL_PARENT_SM_COMPLETE) {
        pass++; printf("PASS: Running -> shutdown -> Complete\n");
    } else { fail++; printf("FAIL: shutdown, r=%d tag=%d\n", r, sm.tag); }

    /* Test 4: child_state_changed guard rejects when child is not Done */
    memset(&sm, 0, sizeof(sm));
    del_parent_sm_init(&sm);
    del_child_sm_init(&sm.running.child);
    r = del_parent_sm_dispatch(&sm,
        &(del_parent_sm_event_t){ .tag = DEL_PARENT_SM_EVENT_CHILD_STATE_CHANGED });
    if (r == WIRESPEC_ERR_INVALID_STATE && sm.tag == DEL_PARENT_SM_RUNNING) {
        pass++; printf("PASS: guard rejects child_state_changed (child=Ready)\n");
    } else { fail++; printf("FAIL: guard should reject, r=%d tag=%d\n", r, sm.tag); }

    printf("\n%d passed, %d failed\n", pass, fail);
    return fail > 0 ? 1 : 0;
}
"#;

    let (ok, stdout, stderr) = compile_and_run(&header, &source, driver, "del");
    eprintln!("=== stdout ===\n{stdout}");
    if !stderr.is_empty() {
        eprintln!("=== stderr ===\n{stderr}");
    }
    assert!(
        ok,
        "SM delegate test failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("0 failed"),
        "Some SM delegate checks failed:\n{stdout}"
    );
}
