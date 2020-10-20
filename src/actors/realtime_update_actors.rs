use crate::actors::{DatasetActor, GetDataset};
use crate::datasets::{
    Dataset, GtfsRT, RealTimeConnection, RealTimeDataset, ScheduleRelationship, UpdatedTimetable,
};
use crate::model_update;
use crate::transit_realtime;
use actix::fut::ActorFuture;
use actix::fut::WrapFuture;
use actix::prelude::ContextFutureSpawner;
use actix::AsyncContext;
use failure::format_err;
use failure::Error;
use prost::Message;
use sentry::integrations::failure::capture_error;
use slog::info;
use std::io::Read;
use std::sync::Arc;

/// Actor that once in a while reload the BaseSchedule data (GTFS)
/// and send them to the DatasetActor
pub struct RealTimeReloader {
    pub gtfs_rt_urls: Vec<String>,
    pub dataset_id: String,

    // Address of the DatasetActor to notify for the data reloading
    // NOte: for the moment it's a single Actor,
    // but if we have several instances of DatasetActor we could have a list of recipient here
    pub dataset_actor: actix::Addr<DatasetActor>,
    pub log: slog::Logger,
}

// TODO: make this async
fn fetch_gtfs_rt(url: &str, log: &slog::Logger) -> Result<GtfsRT, Error> {
    info!(log, "fetching a gtfs_rt");
    let gtfs_rt = reqwest::get(url)
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format_err!("Unable to fetch GTFS: {}", e))
        .and_then(|resp| {
            resp.bytes()
                .collect::<Result<Vec<u8>, _>>()
                .map_err(|e| format_err!("Unable to decode protobuf {}", e))
        });

    match gtfs_rt {
        Ok(gtfs_rt) => Ok(GtfsRT {
            data: gtfs_rt,
            datetime: chrono::Utc::now(),
        }),
        Err(e) => Err(format_err!("Unable to fetch GTFS-RT: {}", e)),
    }
}

fn aggregate_rts(feed_messages: &[transit_realtime::FeedMessage]) -> Result<GtfsRT, Error> {
    //We may loose a timestamp, other fields are ok
    let first = feed_messages
        .first()
        .ok_or_else(|| format_err!("No feed message!"))?;
    let entity = feed_messages
        .iter()
        .map(|fm| fm.entity.clone())
        .flatten()
        .collect();
    let res = transit_realtime::FeedMessage {
        header: first.header.clone(),
        entity,
    };
    let mut data = Vec::new();
    res.encode(&mut data)
        .map_err(|err| format_err!("Unable to encode protobuf: {}", err))?;
    Ok(GtfsRT {
        data,
        datetime: chrono::Utc::now(),
    })
}

// modify the generated timetable with a given GTFS-RT
// Since the connection are sorted by scheduled departure time we don't need to reorder the connections, we can update them in place
// For each trip update, we only have to find the corresponding connection and update it.
fn apply_rt_update(
    data: &Dataset,
    gtfs_rts: &[transit_realtime::FeedMessage],
    log: &slog::Logger,
) -> Result<UpdatedTimetable, Error> {
    let mut updated_timetable = UpdatedTimetable::default();

    let parsed_trip_update = model_update::get_model_update(&data.ntm, gtfs_rts, data.timezone)?;
    let mut nb_changes = 0;
    let mut cpt_incoherent_stops_id = 0;

    for (idx, connection) in data.timetable.connections.iter().enumerate() {
        let trip_update = parsed_trip_update.trips.get(&connection.dated_vj);
        if let Some(trip_update) = trip_update {
            let stop_time_update = trip_update
                .stop_time_update_by_sequence
                .get(&connection.sequence);
            if let Some(stop_time_update) = stop_time_update {
                // integrity check
                if let Some(stop_idx) = stop_time_update.stop_point_idx {
                    if stop_idx != connection.stop_point_idx {
                        slog::warn!(log, "for trip {}, invalid stop connection, the stop n.{} '{}' does not correspond to the gtfsrt stop '{}'",
                    &data.ntm.vehicle_journeys[connection.dated_vj.vj_idx].id,
                    &connection.sequence,
                    &data.ntm.stop_points[connection.stop_point_idx].id,
                    &data.ntm.stop_points[stop_idx].id,
                    );
                        cpt_incoherent_stops_id += 1;
                        continue;
                    }
                }
                updated_timetable.realtime_connections.insert(
                    idx,
                    RealTimeConnection {
                        dep_time: stop_time_update.updated_departure,
                        arr_time: stop_time_update.updated_arrival,
                        schedule_relationship: ScheduleRelationship::Scheduled,
                        update_time: trip_update.update_dt,
                    },
                );
                nb_changes += 1;
            } else {
                continue;
            }
        } else {
            // no trip update for this vehicle journey, we can skip
            continue;
        }
    }
    if cpt_incoherent_stops_id != 0 {
        sentry::capture_message(
            "stop id incoherent with base schedule",
            sentry::Level::Warning,
        );
    }

    info!(
        log,
        "{} connections have been updated with trip updates", nb_changes
    );

    Ok(updated_timetable)
}

impl RealTimeReloader {
    fn update_realtime_data(&self, ctx: &mut actix::Context<Self>) {
        // we fetch the latest baseschedule data
        let dataset_id = self.dataset_id.clone();
        self.dataset_actor
            .send(GetDataset)
            .into_actor(self)
            .then(|res, act, _| {
                sentry::Hub::current().configure_scope(|scope| {
                    scope.set_tag("dataset", dataset_id);
                });
                match res
                    .map_err(|e| format_err!("maibox error: {}", e))
                    .and_then(|dataset| act.apply_rt(dataset))
                {
                    Ok(()) => {
                        info!(act.log, "realtime reloaded");
                    }
                    Err(e) => {
                        slog::error!(act.log, "unable to apply realtime update due to: {}", e);
                        capture_error(&e);
                    }
                }
                actix::fut::ready(())
            })
            .wait(ctx);
    }

    fn apply_rt(&self, dataset: Arc<Dataset>) -> Result<(), Error> {
        //TODO: make this async
        let gtfs_rts = self
            .gtfs_rt_urls
            .iter()
            .filter_map(|url| {
                fetch_gtfs_rt(&url, &self.log)
                    .map_err(|e| slog::warn!(self.log, "{}", e))
                    .ok()
            })
            .collect();

        let rt_dataset = self.make_rt_dataset(dataset, gtfs_rts)?;
        // we send those data as a BaseScheduleReloader message, for the DatasetActor to load those new data
        self.dataset_actor
            .do_send(UpdateRealtime(Arc::new(rt_dataset)));
        Ok(())
    }

    fn make_rt_dataset(
        &self,
        dataset: Arc<Dataset>,
        gtfs_rts: Vec<GtfsRT>,
    ) -> Result<RealTimeDataset, Error> {
        let feed_messages: Vec<transit_realtime::FeedMessage> = gtfs_rts
            .iter()
            .filter_map(GtfsRT::decode_feed_message)
            .collect();

        let gtfs_rt = aggregate_rts(&feed_messages)?;
        let updated_timetable = apply_rt_update(&dataset, &feed_messages, &self.log)?;

        Ok(RealTimeDataset {
            base_schedule_dataset: dataset,
            gtfs_rt: Some(gtfs_rt),
            gtfs_rt_provider_urls: self.gtfs_rt_urls.clone(),
            updated_timetable,
        })
    }
}

impl actix::Actor for RealTimeReloader {
    type Context = actix::Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!(self.log, "Starting the realtime updater actor");

        self.update_realtime_data(ctx);
        ctx.run_interval(std::time::Duration::from_secs(60), |act, ctx| {
            info!(act.log, "reloading realtime data");
            act.update_realtime_data(ctx);
        });
    }
}

/// Message send to a DatasetActor to update its baseschedule data
struct UpdateRealtime(Arc<RealTimeDataset>);

impl actix::Message for UpdateRealtime {
    type Result = ();
}

impl actix::Handler<UpdateRealtime> for DatasetActor {
    type Result = ();

    fn handle(&mut self, params: UpdateRealtime, _ctx: &mut actix::Context<Self>) -> Self::Result {
        self.realtime = params.0;
    }
}
