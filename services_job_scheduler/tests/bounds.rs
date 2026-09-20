//! Priority, cancellation, progress and history bounds.

use services_job_scheduler::*;

#[test]
fn a_yielding_job_does_not_starve_a_higher_priority_one() {
    let mut scheduler = JobScheduler::new();
    scheduler.schedule_job(JobDescriptor::new(
        "indexer",
        JobPriority::Low,
        Box::new(|_| JobResult::Yielded),
    ));
    scheduler.tick();

    let urgent = scheduler.schedule_job(JobDescriptor::new(
        "urgent",
        JobPriority::High,
        Box::new(|_| JobResult::Completed),
    ));
    for _ in 0..4 {
        scheduler.tick();
    }
    assert_eq!(
        scheduler.get_job_status(urgent),
        Some(JobStatus::Completed),
        "the High-priority job is still {:?}: priority was honoured at \
         insertion and nowhere else",
        scheduler.get_job_status(urgent)
    );
}

#[test]
fn a_running_job_can_be_cancelled() {
    let mut scheduler = JobScheduler::new();
    let looping = scheduler.schedule_job(JobDescriptor::new(
        "looping",
        JobPriority::Normal,
        Box::new(|_| JobResult::Yielded),
    ));
    scheduler.tick();
    assert_eq!(scheduler.get_job_status(looping), Some(JobStatus::Yielded));

    assert!(
        scheduler.cancel_job(looping),
        "the one job that most needs stopping could not be"
    );
    assert_eq!(
        scheduler.get_job_status(looping),
        Some(JobStatus::Cancelled)
    );
}

#[test]
fn a_job_can_report_its_own_progress() {
    let mut scheduler = JobScheduler::new();
    let job = scheduler.schedule_job(JobDescriptor::new(
        "slow",
        JobPriority::Normal,
        Box::new(|ctx| {
            ctx.progress = (ctx.job_ticks * 10).min(90) as u8;
            if ctx.job_ticks >= 5 {
                JobResult::Completed
            } else {
                JobResult::Yielded
            }
        }),
    ));
    for _ in 0..3 {
        scheduler.tick();
    }
    let progress = scheduler.get_job_progress(job).unwrap();
    assert!(
        progress > 0 && progress < 100,
        "a running job reported {progress}: `set_progress` had no route from \
         a job to its own descriptor"
    );
}

#[test]
fn completed_job_history_is_bounded() {
    let mut scheduler = JobScheduler::new();
    for i in 0..(JobScheduler::MAX_COMPLETED_HISTORY * 2) {
        scheduler.schedule_job(JobDescriptor::new(
            format!("job_{i}"),
            JobPriority::Normal,
            Box::new(|_| JobResult::Completed),
        ));
        scheduler.tick();
    }
    assert!(
        scheduler.completed_count() <= JobScheduler::MAX_COMPLETED_HISTORY,
        "the history holds {} jobs",
        scheduler.completed_count()
    );
}
