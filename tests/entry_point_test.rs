mod utils;

#[actix_rt::test]
async fn entry_point_integration_test() {
    let _log_guard = utils::init_log();
    let mut srv = utils::make_simple_test_server().await;

    test_entrypoint(&mut srv).await;
    test_dataset_entrypoint(&mut srv).await;
    test_siri_entrypoint(&mut srv).await;
}

async fn test_entrypoint(srv: &mut actix_web::test::TestServer) {
    let resp: serde_json::Value = utils::get_json(srv, "/").await;
    assert_eq!(
        resp,
        serde_json::json! {
                {
                "_links": {
                    "dataset_detail": {
                        "href": &srv.url("/{id}/"),
                        "templated": true
                    },
                    "documentation": {
                        "href": &srv.url("/spec")
                    }
                },
                "datasets": [
                    {
                        "_links": {
                            "self": {
                                "href": &srv.url("/default/")
                            }
                        },
                        "gtfs": "fixtures/gtfs.zip",
                        "id": "default",
                        "name": "default name",
                        "extras": {},
                    }
                ]
            }
        }
    );
}

async fn test_dataset_entrypoint(srv: &mut actix_web::test::TestServer) {
    let mut resp: serde_json::Value = utils::get_json(srv, "/default/").await;

    // we change the loaded_at datetime to be able to easily compare the response
    *resp.pointer_mut("/loaded_at").unwrap() = "2019-06-20T10:00:00Z".into();
    assert_eq!(
        resp,
        serde_json::json! {
            {
                "name": "default name",
                "id": "default",
                "gtfs": "fixtures/gtfs.zip",
                "loaded_at": "2019-06-20T10:00:00Z",
                "extras": {},
                "_links": {
                    "general-message": {
                        "href": &srv.url("/default/siri/2.0/general-message.json")
                    },
                    "gtfs-rt": {
                        "href": &srv.url("/default/gtfs-rt")
                    },
                    "gtfs-rt.json": {
                        "href": &srv.url("/default/gtfs-rt.json")
                    },
                    "stop-monitoring": {
                        "href": &srv.url("/default/siri/2.0/stop-monitoring.json")
                    },
                    "siri-lite": {
                        "href": &srv.url("/default/siri/2.0")
                    },
                    "stoppoints-discovery": {
                        "href": &srv.url("/default/siri/2.0/stoppoints-discovery.json")
                    }
                }
            }
        }
    );
}

async fn test_siri_entrypoint(srv: &mut actix_web::test::TestServer) {
    let resp: serde_json::Value = utils::get_json(srv, "/default/siri/2.0").await;

    assert_eq!(
        resp,
        serde_json::json! {
            {
                "_links": {
                    "general-message": {
                        "href": &srv.url("/default/siri/2.0/general-message.json")
                    },
                    "stop-monitoring": {
                        "href": &srv.url("/default/siri/2.0/stop-monitoring.json")
                    },
                    "stoppoints-discovery": {
                        "href": &srv.url("/default/siri/2.0/stoppoints-discovery.json")
                    }
                }
            }
        }
    );
}
