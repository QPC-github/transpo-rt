use maplit::btreeset;
use std::collections::BTreeSet;
mod utils;

#[actix_rt::test]
async fn openapi_test() {
    let _log_guard = utils::init_log();
    let mut srv = utils::make_simple_test_server();
    let resp: serde_json::Value = utils::get_json(&mut srv, "/spec").await;
    let paths = resp.pointer("/paths").unwrap();
    assert_eq!(
        paths
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect::<BTreeSet<&str>>(),
        btreeset! {
            "/",
            "/spec",
            "/{dataset}",
            "/{dataset}/gtfs-rt",
            "/{dataset}/gtfs-rt.json",
            "/{dataset}/siri/2.0/general-message.json",
            "/{dataset}/siri/2.0/stop-monitoring.json",
            "/{dataset}/siri/2.0/stoppoints-discovery.json",
        }
    );

    let nb_params = |route: &str| {
        let route = route.replace("/", "~1");
        let pointer = format!("/paths/{}/get/parameters", route);
        resp.pointer(&pointer)
            .map(|p| p.as_array().unwrap().len())
            .unwrap_or(0)
    };

    assert_eq!(nb_params("/"), 0);
    assert_eq!(nb_params("/spec"), 0);
    assert_eq!(nb_params("/{dataset}"), 1);
    assert_eq!(nb_params("/{dataset}/gtfs-rt"), 1);
    assert_eq!(nb_params("/{dataset}/gtfs-rt.json"), 1);
    assert_eq!(nb_params("/{dataset}/siri/2.0/general-message.json"), 2);
    assert_eq!(nb_params("/{dataset}/siri/2.0/stop-monitoring.json"), 7);
    assert_eq!(
        nb_params("/{dataset}/siri/2.0/stoppoints-discovery.json"),
        8
    );

    // we don't check all the responses, just that there is at least the definition of SiriResponse
    resp.pointer("/components/schemas/SiriResponse")
        .expect("impossible to find SiriResponse");
}
