use super::progress::StyledProgressBar;
use colored::Colorize;
use indicatif::MultiProgress;
use log::{error, info, warn};
use mutant_client::ProgressReceiver;
use mutant_protocol::{PutCallback, PutEvent, TaskProgress};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
struct PutCallbackContext {
    res_pb_opt: Arc<Mutex<Option<StyledProgressBar>>>,
    upload_pb_opt: Arc<Mutex<Option<StyledProgressBar>>>,
    confirm_pb_opt: Arc<Mutex<Option<StyledProgressBar>>>,
    multi_progress: MultiProgress,
    total_chunks: Arc<Mutex<usize>>,
    first_complete_seen: Arc<Mutex<bool>>,
    start_time: Arc<Mutex<std::time::Instant>>,
}

impl PutCallbackContext {
    /// Finishes and clears a progress bar, setting it to 100% if not already finished.
    /// Returns after the progress bar is cleared.
    async fn finish_progress_bar(
        &self,
        pb_mutex: &Arc<Mutex<Option<StyledProgressBar>>>,
        completion_message: &str,
        bar_name: &str,
    ) {
        let mut pb_guard = pb_mutex.lock().await;
        if let Some(pb) = pb_guard.take() {
            info!("Clearing {} bar", bar_name);
            if !pb.is_finished() {
                // If the bar isn't at 100%, set it to 100% before clearing
                if let Some(len) = pb.length() {
                    pb.set_position(len);
                }
                pb.set_message(completion_message.to_string());
            }
            pb.finish_and_clear();
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn create_put_progress(mut progress_rx: ProgressReceiver, multi_progress: MultiProgress) {
    let res_pb_opt = Arc::new(Mutex::new(None::<StyledProgressBar>));
    let upload_pb_opt = Arc::new(Mutex::new(None::<StyledProgressBar>));
    let confirm_pb_opt = Arc::new(Mutex::new(None::<StyledProgressBar>));
    let total_chunks_arc = Arc::new(Mutex::new(0usize));
    let first_complete_seen = Arc::new(Mutex::new(false));
    let start_time = Arc::new(Mutex::new(std::time::Instant::now()));

    let context = PutCallbackContext {
        res_pb_opt: res_pb_opt.clone(),
        upload_pb_opt: upload_pb_opt.clone(),
        confirm_pb_opt: confirm_pb_opt.clone(),
        multi_progress: multi_progress.clone(),
        total_chunks: total_chunks_arc.clone(),
        first_complete_seen: first_complete_seen.clone(),
        start_time: start_time.clone(),
    };

    let ctx_clone = context.clone();

    let callback: PutCallback = Arc::new(move |event: PutEvent| {
        let ctx = ctx_clone.clone();

        Box::pin(async move {
            match event {
                PutEvent::Starting {
                    total_chunks,
                    initial_written_count,
                    initial_confirmed_count,
                    chunks_to_reserve,
                } => {
                    info!(
                        "Starting put operation - Total: {}, Written: {}, Confirmed: {}, To Reserve: {}",
                        total_chunks, initial_written_count, initial_confirmed_count, chunks_to_reserve
                    );
                    *ctx.total_chunks.lock().await = total_chunks;
                    let total_u64 = total_chunks as u64;

                    if chunks_to_reserve > 0 {
                        let mut res_pb_guard = ctx.res_pb_opt.lock().await;
                        let res_pb = res_pb_guard.get_or_insert_with(|| {
                            info!("Creating reservation progress bar");
                            let pb = StyledProgressBar::new_for_steps(&ctx.multi_progress);
                            pb.set_message("Buying new pads...".to_string());
                            pb
                        });
                        info!("Setting reservation bar length to {}", chunks_to_reserve);
                        res_pb.set_length(chunks_to_reserve as u64);
                        res_pb.set_position(0);
                        drop(res_pb_guard);
                    }

                    let mut upload_pb_guard = ctx.upload_pb_opt.lock().await;
                    let upload_pb = upload_pb_guard.get_or_insert_with(|| {
                        info!("Creating upload progress bar");
                        let pb = StyledProgressBar::new_for_steps(&ctx.multi_progress);
                        pb.set_message("Uploading pads...".to_string());
                        pb
                    });
                    info!("Setting upload bar length to {}", total_u64);
                    upload_pb.set_length(total_u64);
                    upload_pb.set_position(initial_written_count as u64);
                    drop(upload_pb_guard);

                    let mut confirm_pb_guard = ctx.confirm_pb_opt.lock().await;
                    let confirm_pb = confirm_pb_guard.get_or_insert_with(|| {
                        info!("Creating confirmation progress bar");
                        let pb = StyledProgressBar::new_for_steps(&ctx.multi_progress);
                        pb.set_message("Confirming pads...".to_string());
                        pb
                    });
                    info!("Setting confirmation bar length to {}", total_u64);
                    confirm_pb.set_length(total_u64);
                    confirm_pb.set_position(initial_confirmed_count as u64);
                    drop(confirm_pb_guard);

                    Ok::<bool, Box<dyn std::error::Error + Send + Sync>>(true)
                }
                PutEvent::PadReserved => {
                    info!("Pad reserved event received");
                    let mut res_pb_guard = ctx.res_pb_opt.lock().await;
                    if let Some(pb) = res_pb_guard.as_mut() {
                        if !pb.is_finished() {
                            info!("Incrementing reservation bar");
                            pb.inc(1);

                            if pb.position() >= pb.length().unwrap_or(0) {
                                info!("All pads acquired");
                                pb.set_message("Pads acquired.".to_string());
                            }
                        }
                    } else {
                        warn!("PadReserved event but acquisition bar doesn't exist");
                    }
                    drop(res_pb_guard);
                    Ok::<bool, Box<dyn std::error::Error + Send + Sync>>(true)
                }
                PutEvent::PadsWritten => {
                    info!("Pads written event received");
                    let mut upload_pb_guard = ctx.upload_pb_opt.lock().await;
                    if let Some(pb) = upload_pb_guard.as_mut() {
                        if !pb.is_finished() {
                            info!("Incrementing upload bar");
                            pb.inc(1);
                        }
                    } else {
                        warn!("PadsWritten event but upload bar doesn't exist");
                    }
                    drop(upload_pb_guard);
                    Ok::<bool, Box<dyn std::error::Error + Send + Sync>>(true)
                }
                PutEvent::PadsConfirmed => {
                    info!("Pads confirmed event received");
                    let mut confirm_pb_guard = ctx.confirm_pb_opt.lock().await;
                    if let Some(pb) = confirm_pb_guard.as_mut() {
                        if !pb.is_finished() {
                            info!("Incrementing confirmation bar");
                            pb.inc(1);
                        }
                    } else {
                        warn!("PadsConfirmed event but confirmation bar doesn't exist");
                    }
                    drop(confirm_pb_guard);
                    Ok::<bool, Box<dyn std::error::Error + Send + Sync>>(true)
                }
                PutEvent::Complete => {
                    // Check if this is the first or second Complete event
                    let mut first_complete_seen_guard = ctx.first_complete_seen.lock().await;
                    let is_first_complete = !*first_complete_seen_guard;

                    if is_first_complete {
                        info!("First Complete event received for data pads");
                        // Mark that we've seen the first Complete event
                        *first_complete_seen_guard = true;
                        drop(first_complete_seen_guard);

                        // Clear all progress bars before showing the message
                        // First, finish the reservation bar if it exists
                        ctx.finish_progress_bar(&ctx.res_pb_opt, "Pads acquired.", "reservation").await;

                        // Next, finish the upload bar
                        ctx.finish_progress_bar(&ctx.upload_pb_opt, "Upload complete.", "upload").await;

                        // Finally, finish the confirmation bar
                        ctx.finish_progress_bar(&ctx.confirm_pb_opt, "Confirmation complete.", "confirmation").await;

                        // Ensure all progress bars are cleared
                        crate::utils::ensure_progress_cleared(&ctx.multi_progress);

                        // Calculate elapsed time
                        let start_time = *ctx.start_time.lock().await;
                        let elapsed = start_time.elapsed();

                        // Format the elapsed time
                        let time_str = crate::utils::format_elapsed_time(elapsed);

                        // Display message about data pads being uploaded
                        let total_chunks = *ctx.total_chunks.lock().await;
                        println!("{} {} data pads have been uploaded (took {})",
                            "•".bright_green(),
                            total_chunks,
                            time_str);

                        return Ok::<bool, Box<dyn std::error::Error + Send + Sync>>(true);
                    }

                    // This is the second Complete event (or the only one for private keys)
                    info!("Final Complete event received, clearing progress bars");
                    drop(first_complete_seen_guard);

                    // Make sure all progress bars are finished and cleared
                    // First, finish the reservation bar if it exists
                    ctx.finish_progress_bar(&ctx.res_pb_opt, "Pads acquired.", "reservation").await;

                    // Next, finish the upload bar
                    ctx.finish_progress_bar(&ctx.upload_pb_opt, "Upload complete.", "upload").await;

                    // Finally, finish the confirmation bar
                    ctx.finish_progress_bar(&ctx.confirm_pb_opt, "Confirmation complete.", "confirmation").await;

                    Ok::<bool, Box<dyn std::error::Error + Send + Sync>>(true)
                }
            }
        })
    });

    tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            match progress {
                Ok(TaskProgress::Put(event)) => {
                    callback(event.clone()).await.unwrap();
                }
                Ok(_) => warn!("Unexpected progress type"),
                Err(e) => error!("Progress error: {:?}", e),
            }
        }
    });
}
