//! A token anybody can construct proves nothing, and a journal that only
//! grows is not a journal.

use services_storage::{
    Capability, CapabilityKind, JournaledStorage, ObjectId, Ownership, PermissionChecker,
    PrincipalId, TransactionalStorage,
};

/// A7's defect, in the crate A7's fix never touched. `Capability::new` is
/// public and takes the object, the kind and the holder as plain arguments,
/// and `check_access` compared only the fields of the struct it was handed --
/// so anyone could mint themselves `Own` on another principal's object. The
/// type doc calls it "an unforgeable capability token".
#[test]
fn a_capability_cannot_simply_be_minted() {
    let object = ObjectId::new();
    let victim = PrincipalId::new();
    let attacker = PrincipalId::new();

    let mut checker = PermissionChecker::new();
    checker.register_object(object, Ownership::new(victim, 0));

    let forged = Capability::new(object, CapabilityKind::Own, attacker);
    assert!(
        checker
            .check_access(&forged, object, CapabilityKind::Write, attacker)
            .is_err(),
        "a capability anyone can construct was accepted as proof of Own on \
         another principal's object"
    );

    // A token the checker issued is accepted, so this has not become a
    // refusal of everything.
    let issued = checker.issue(object, CapabilityKind::Own, victim);
    checker
        .check_access(&issued, object, CapabilityKind::Write, victim)
        .expect("a capability this checker issued must be honoured");

    // And revoking it withdraws the authority.
    assert!(checker.revoke(&issued));
    assert!(
        checker
            .check_access(&issued, object, CapabilityKind::Write, victim)
            .is_err(),
        "a revoked capability still granted access"
    );
}

/// R3 freed superseded versions in the block layer and left this backend --
/// the storage `pandagend` hands the editor. Two hundred saves of a 64 KiB
/// file retained thirteen megabytes of journal, on a kernel whose heap floor
/// is twelve.
#[test]
fn rewriting_one_file_does_not_grow_without_bound() {
    let mut storage = JournaledStorage::new();
    let object = ObjectId::new();
    let payload = vec![7u8; 64 * 1024];

    for _ in 0..200 {
        let mut tx = storage.begin_transaction().unwrap();
        storage.write(&mut tx, object, &payload).unwrap();
        storage.commit(&mut tx).unwrap();
    }

    let retained: usize = storage
        .journal_entries()
        .iter()
        .map(|entry| match entry {
            services_storage::journaled_storage::JournalEntry::Write { data, .. } => data.len(),
            _ => 0,
        })
        .sum();
    assert!(
        retained <= 8 * payload.len(),
        "200 saves of a 64 KiB file retained {retained} bytes of journal"
    );

    // And the file is still exactly what was last written -- compaction
    // must not lose the state it is compacting.
    let mut tx = storage.begin_transaction().unwrap();
    assert_eq!(storage.read_data(&tx, object).unwrap(), payload);
    let _ = storage.rollback(&mut tx);

    // Including across a recovery, which replays the journal from scratch.
    storage.recover();
    let mut tx = storage.begin_transaction().unwrap();
    assert_eq!(
        storage.read_data(&tx, object).unwrap(),
        payload,
        "compaction lost the state a recovery needs"
    );
    let _ = storage.rollback(&mut tx);
}
