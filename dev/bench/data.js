window.BENCHMARK_DATA = {
  "lastUpdate": 1777570369155,
  "repoUrl": "https://github.com/iii-hq/iii",
  "entries": {
    "iii Engine Benchmarks": [
      {
        "commit": {
          "author": {
            "name": "Guilherme Beira",
            "username": "guibeira",
            "email": "guilherme.vieira.beira@gmail.com"
          },
          "committer": {
            "name": "Guilherme Beira",
            "username": "guibeira",
            "email": "guilherme.vieira.beira@gmail.com"
          },
          "id": "811393dfa29bfc5efebae2abb502862c7ad9e30b",
          "message": "fix: use --bench '*' instead of --benches in benchmark workflow\n\n--benches includes the lib target which uses the default test harness\nand doesn't support --output-format bencher (a Criterion flag). Using\n--bench '*' runs only the bench targets in benches/ which all use\nCriterion with harness = false.",
          "timestamp": "2026-03-11T16:57:57Z",
          "url": "https://github.com/iii-hq/iii/commit/811393dfa29bfc5efebae2abb502862c7ad9e30b"
        },
        "date": 1773249756485,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2961,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 21559,
            "range": "± 435",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 89757,
            "range": "± 1080",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 365265,
            "range": "± 2845",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 42506,
            "range": "± 1436",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 158887,
            "range": "± 501",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 85698,
            "range": "± 1009",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 447912,
            "range": "± 5309",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4395639,
            "range": "± 34957",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 845987,
            "range": "± 2311",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2271485,
            "range": "± 12495",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25572172,
            "range": "± 925926",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4264016,
            "range": "± 10214",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2318,
            "range": "± 41",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 537448,
            "range": "± 5212",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42241400,
            "range": "± 308500",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46536290,
            "range": "± 501009",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56390284,
            "range": "± 4440483",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 333306,
            "range": "± 11501",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 352367,
            "range": "± 8348",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 533931,
            "range": "± 10687",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2466970,
            "range": "± 118745",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 338864,
            "range": "± 12302",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1916,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2663,
            "range": "± 32",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9052,
            "range": "± 63",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 106814,
            "range": "± 1519",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1132,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 348,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 103,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1017,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 578,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 567,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1191,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 374,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1471,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 5773,
            "range": "± 44",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 29564,
            "range": "± 457",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38442,
            "range": "± 389",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 433,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 281,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 244,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 240,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 435,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 455,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 277,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 91,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 106,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 105,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 774,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 800,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1387,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1514,
            "range": "± 29",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 615,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 221,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 127,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 126,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1038,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1055,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1838,
            "range": "± 44",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2046,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 909,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 283,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 35621,
            "range": "± 847",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58137,
            "range": "± 588",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 110775,
            "range": "± 6820",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 328264,
            "range": "± 10470",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 7317,
            "range": "± 192",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 7282,
            "range": "± 86",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 25041,
            "range": "± 314",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 97996,
            "range": "± 1725",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 394340,
            "range": "± 4955",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6363,
            "range": "± 48",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7238,
            "range": "± 106",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50812,
            "range": "± 230",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 152294,
            "range": "± 3188",
            "unit": "ns/iter"
          },
          {
            "name": "startup/build_and_destroy_engine_from_minimal_config",
            "value": 98748,
            "range": "± 2812",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1415,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 384,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 127,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1423,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 431,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38110,
            "range": "± 96",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 103,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 35745,
            "range": "± 984",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 59529,
            "range": "± 654",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 122698,
            "range": "± 3094",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 430949,
            "range": "± 10009",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 7507,
            "range": "± 3221",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 29836,
            "range": "± 1530",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 111135,
            "range": "± 4342",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2139,
            "range": "± 369",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3173,
            "range": "± 213",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7011,
            "range": "± 195",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7696,
            "range": "± 445",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 27695,
            "range": "± 779",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 95897,
            "range": "± 2838",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 834,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 1022,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 6959,
            "range": "± 104",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 33662,
            "range": "± 74",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 136223,
            "range": "± 520",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 846,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 178638,
            "range": "± 10055",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "accfd96f6a8e9bd50a3957efc08703ccfd40e0cb",
          "message": "chore: bump versions for release -- iii(iii/v0.11.1-next.1)",
          "timestamp": "2026-04-20T12:21:49Z",
          "tree_id": "7a0d06d458acdf38e1f59efe45e985137f1af257",
          "url": "https://github.com/iii-hq/iii/commit/accfd96f6a8e9bd50a3957efc08703ccfd40e0cb"
        },
        "date": 1776689424143,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2789,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18302,
            "range": "± 344",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 81253,
            "range": "± 385",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 322791,
            "range": "± 1973",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45725,
            "range": "± 1447",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 132766,
            "range": "± 432",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 84665,
            "range": "± 1269",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 459704,
            "range": "± 934",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3743389,
            "range": "± 24041",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 859031,
            "range": "± 4058",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2401305,
            "range": "± 8343",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 20686868,
            "range": "± 405405",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4326719,
            "range": "± 12155",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2105,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 544617,
            "range": "± 16435",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42074156,
            "range": "± 124968",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45648901,
            "range": "± 2169913",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58523357,
            "range": "± 2527885",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 322064,
            "range": "± 6441",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 346442,
            "range": "± 9969",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 542006,
            "range": "± 17233",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2277101,
            "range": "± 55373",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 330441,
            "range": "± 11105",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1652,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2321,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9021,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 104300,
            "range": "± 653",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1097,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 348,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 105,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 997,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 585,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 565,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1171,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 381,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1478,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 5878,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 30419,
            "range": "± 79",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37125,
            "range": "± 110",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 445,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 282,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 235,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 241,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 392,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 406,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 248,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 79,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 109,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 109,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 787,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 793,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1346,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1867,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 604,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 211,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 130,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 135,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1036,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1033,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1813,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1905,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 866,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 282,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 37266,
            "range": "± 1536",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 56802,
            "range": "± 401",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 100841,
            "range": "± 5723",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 299533,
            "range": "± 18384",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8086,
            "range": "± 149",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8132,
            "range": "± 172",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27745,
            "range": "± 128",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 107642,
            "range": "± 476",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 436286,
            "range": "± 2473",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6970,
            "range": "± 58",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7864,
            "range": "± 210",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 51482,
            "range": "± 95",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 122208,
            "range": "± 300",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1385,
            "range": "± 108",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 381,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 124,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1453,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 418,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37143,
            "range": "± 480",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 104,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 36845,
            "range": "± 1857",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 56615,
            "range": "± 440",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 125183,
            "range": "± 2806",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 432444,
            "range": "± 8036",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4983,
            "range": "± 388",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16010,
            "range": "± 200",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56937,
            "range": "± 4132",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2137,
            "range": "± 98",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3103,
            "range": "± 73",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6910,
            "range": "± 151",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7559,
            "range": "± 841",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26911,
            "range": "± 756",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 93173,
            "range": "± 2921",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 549,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 706,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4883,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24977,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101608,
            "range": "± 443",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 550,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 212057,
            "range": "± 4637",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "e33a3e4d7e40ffea51f50e68fb2bfe139d734ed0",
          "message": "chore: bump versions for release -- iii(iii/v0.11.1)",
          "timestamp": "2026-04-20T12:55:14Z",
          "tree_id": "4ff70be48e99585624cecf6eb299317945f51ac0",
          "url": "https://github.com/iii-hq/iii/commit/e33a3e4d7e40ffea51f50e68fb2bfe139d734ed0"
        },
        "date": 1776691417476,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3089,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20984,
            "range": "± 478",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 92001,
            "range": "± 196",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 363650,
            "range": "± 1080",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 49766,
            "range": "± 1951",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 143559,
            "range": "± 759",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88542,
            "range": "± 661",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 488383,
            "range": "± 5491",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4471141,
            "range": "± 21189",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 870372,
            "range": "± 5753",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2538552,
            "range": "± 50928",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25366724,
            "range": "± 1141169",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4438878,
            "range": "± 88074",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2295,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 546118,
            "range": "± 25866",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42182036,
            "range": "± 244890",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 42789240,
            "range": "± 2844461",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 55995828,
            "range": "± 9764285",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 320385,
            "range": "± 13657",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 348260,
            "range": "± 4262",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 532902,
            "range": "± 6208",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2490244,
            "range": "± 101702",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 319917,
            "range": "± 8720",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1864,
            "range": "± 67",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2495,
            "range": "± 38",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9304,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86211,
            "range": "± 214",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1201,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 360,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 107,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1106,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 607,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 585,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1226,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 373,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1488,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 5958,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 30383,
            "range": "± 189",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 40039,
            "range": "± 1743",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 397,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 291,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 39,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 236,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 230,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 400,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 404,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 249,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 87,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 109,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 110,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 897,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 889,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1519,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1619,
            "range": "± 100",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 682,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 217,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 123,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 123,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1136,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1118,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2024,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2129,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 934,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 287,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33572,
            "range": "± 1571",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52619,
            "range": "± 565",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 99623,
            "range": "± 4020",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 323159,
            "range": "± 15482",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8502,
            "range": "± 166",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8553,
            "range": "± 166",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29362,
            "range": "± 234",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 116094,
            "range": "± 268",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 463045,
            "range": "± 5636",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7428,
            "range": "± 50",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8416,
            "range": "± 124",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17561,
            "range": "± 147",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 124019,
            "range": "± 785",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1468,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 395,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 130,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1422,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 446,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 39550,
            "range": "± 130",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 108,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 33615,
            "range": "± 483",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 55767,
            "range": "± 409",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 127007,
            "range": "± 1397",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 457464,
            "range": "± 7411",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5340,
            "range": "± 537",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18045,
            "range": "± 515",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 63710,
            "range": "± 3109",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2289,
            "range": "± 107",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3315,
            "range": "± 123",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7276,
            "range": "± 218",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7850,
            "range": "± 325",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28524,
            "range": "± 417",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 98123,
            "range": "± 3326",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 671,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 811,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5844,
            "range": "± 176",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29505,
            "range": "± 65",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 121712,
            "range": "± 768",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 677,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 167464,
            "range": "± 4648",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "a0e54033feab388a54ed9377bdc1c8c06fc8b2df",
          "message": "chore: bump versions for release -- iii(iii/v0.11.2-next.1)",
          "timestamp": "2026-04-20T22:16:32Z",
          "tree_id": "69dd8ee7ddec265be112b06caa805fe949ef59dc",
          "url": "https://github.com/iii-hq/iii/commit/a0e54033feab388a54ed9377bdc1c8c06fc8b2df"
        },
        "date": 1776725091620,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3161,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20713,
            "range": "± 221",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 93740,
            "range": "± 1616",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 375651,
            "range": "± 2600",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 46682,
            "range": "± 3969",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 137896,
            "range": "± 7523",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88311,
            "range": "± 423",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 485347,
            "range": "± 1320",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4469142,
            "range": "± 46252",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 869172,
            "range": "± 6301",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2533855,
            "range": "± 12318",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25334791,
            "range": "± 707562",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4443796,
            "range": "± 16580",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2399,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 535211,
            "range": "± 7729",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42152917,
            "range": "± 234105",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 44983916,
            "range": "± 2342889",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56589619,
            "range": "± 3388486",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 316903,
            "range": "± 3160",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 345208,
            "range": "± 3095",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 540796,
            "range": "± 12478",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2545730,
            "range": "± 96450",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 315075,
            "range": "± 2890",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1898,
            "range": "± 90",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2548,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9557,
            "range": "± 76",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86345,
            "range": "± 571",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1175,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 386,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 111,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1072,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 597,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 582,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1257,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 387,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1543,
            "range": "± 93",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 5987,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 31358,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 39665,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 448,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 296,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 220,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 221,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 379,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 402,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 237,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 79,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 113,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 109,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 858,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 851,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1470,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1643,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 639,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 228,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 127,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 127,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1088,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1043,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1953,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2067,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 888,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 278,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33220,
            "range": "± 593",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52094,
            "range": "± 532",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 99634,
            "range": "± 3942",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 319324,
            "range": "± 11816",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8539,
            "range": "± 185",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8575,
            "range": "± 115",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29192,
            "range": "± 1026",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 114257,
            "range": "± 363",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 459900,
            "range": "± 9534",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7254,
            "range": "± 45",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8264,
            "range": "± 64",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17542,
            "range": "± 64",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123783,
            "range": "± 1646",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1467,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 394,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 134,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1525,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 531,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 40275,
            "range": "± 193",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 110,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 33239,
            "range": "± 1329",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 54978,
            "range": "± 426",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 118369,
            "range": "± 3593",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 453497,
            "range": "± 17530",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5365,
            "range": "± 577",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 17934,
            "range": "± 1079",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 62969,
            "range": "± 2582",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2262,
            "range": "± 138",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3306,
            "range": "± 159",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7521,
            "range": "± 205",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7821,
            "range": "± 244",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28779,
            "range": "± 464",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 97146,
            "range": "± 3075",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 659,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 802,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5845,
            "range": "± 49",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 30039,
            "range": "± 116",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 124424,
            "range": "± 543",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 680,
            "range": "± 4",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "20234911fba336036e2d64c00ec4da7a9d136ba4",
          "message": "chore: bump versions for release -- iii(iii/v0.11.2-next.2)",
          "timestamp": "2026-04-20T23:22:21Z",
          "tree_id": "3a21ef7913fa8fa93c37079ae2056e9bb3224a71",
          "url": "https://github.com/iii-hq/iii/commit/20234911fba336036e2d64c00ec4da7a9d136ba4"
        },
        "date": 1776728122781,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2536,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 16775,
            "range": "± 161",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 74600,
            "range": "± 569",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 295516,
            "range": "± 1565",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 48149,
            "range": "± 1425",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 145218,
            "range": "± 853",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 90924,
            "range": "± 1481",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 480793,
            "range": "± 621",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 2969213,
            "range": "± 18742",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 905883,
            "range": "± 2358",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2526389,
            "range": "± 39236",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 17927891,
            "range": "± 802049",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4684424,
            "range": "± 17821",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 1939,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 576443,
            "range": "± 37235",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42490557,
            "range": "± 259518",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45429010,
            "range": "± 2622441",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 59345641,
            "range": "± 4039058",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "c015e59df75eddb540a14136d605daef5531ebff",
          "message": "chore: bump versions for release -- iii(iii/v0.11.2-next.3)",
          "timestamp": "2026-04-21T00:53:45Z",
          "tree_id": "606e915c487c84544f7f3bf4e6545b7dec3a02ad",
          "url": "https://github.com/iii-hq/iii/commit/c015e59df75eddb540a14136d605daef5531ebff"
        },
        "date": 1776734537945,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2808,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18796,
            "range": "± 636",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 82881,
            "range": "± 345",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 334106,
            "range": "± 1372",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44778,
            "range": "± 1034",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 125721,
            "range": "± 751",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86511,
            "range": "± 1234",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 460809,
            "range": "± 1578",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3773303,
            "range": "± 24807",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 873015,
            "range": "± 2274",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2360536,
            "range": "± 15641",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21332235,
            "range": "± 587710",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4399340,
            "range": "± 13708",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2130,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 568337,
            "range": "± 15429",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42150891,
            "range": "± 141662",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45796056,
            "range": "± 1629074",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 59253299,
            "range": "± 3121680",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 366437,
            "range": "± 16727",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 373954,
            "range": "± 10180",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 569734,
            "range": "± 20637",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2355613,
            "range": "± 69283",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 355218,
            "range": "± 16595",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1644,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2261,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8669,
            "range": "± 29",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 105698,
            "range": "± 481",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1091,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 348,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 104,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1010,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 585,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 557,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1173,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 377,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1514,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6030,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 29462,
            "range": "± 59",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37626,
            "range": "± 370",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 384,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 283,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 225,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 248,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 450,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 447,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 284,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 91,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 103,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 790,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 832,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1627,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1502,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 621,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 220,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 135,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 135,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1069,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1059,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1863,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2065,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 917,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 285,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41659,
            "range": "± 1340",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58565,
            "range": "± 1753",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 105618,
            "range": "± 4043",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 303336,
            "range": "± 12045",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8130,
            "range": "± 175",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8199,
            "range": "± 146",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27549,
            "range": "± 166",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 106889,
            "range": "± 355",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 433644,
            "range": "± 1001",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7063,
            "range": "± 62",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8033,
            "range": "± 89",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50755,
            "range": "± 172",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123616,
            "range": "± 639",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1406,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 390,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 126,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1499,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 429,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37226,
            "range": "± 159",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 109,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 41161,
            "range": "± 1994",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 63112,
            "range": "± 478",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 128788,
            "range": "± 4063",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 440445,
            "range": "± 6078",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4977,
            "range": "± 408",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16074,
            "range": "± 186",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56011,
            "range": "± 2208",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2113,
            "range": "± 44",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3084,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6737,
            "range": "± 155",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7315,
            "range": "± 128",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 27054,
            "range": "± 241",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 93962,
            "range": "± 2930",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 542,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 682,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4794,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24977,
            "range": "± 85",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101821,
            "range": "± 460",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 549,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 225807,
            "range": "± 6668",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "2b445957701f94dc5f56f900af314e9d59f3b0f7",
          "message": "chore: bump versions for release -- iii(iii/v0.11.2)",
          "timestamp": "2026-04-21T01:19:23Z",
          "tree_id": "fd9eac5ca8b1a0d7e5e6caebac5b959db936970f",
          "url": "https://github.com/iii-hq/iii/commit/2b445957701f94dc5f56f900af314e9d59f3b0f7"
        },
        "date": 1776735340041,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3148,
            "range": "± 72",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20907,
            "range": "± 535",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 89332,
            "range": "± 1345",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 356782,
            "range": "± 926",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 47533,
            "range": "± 1712",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 143343,
            "range": "± 375",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88161,
            "range": "± 571",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 485033,
            "range": "± 854",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4475434,
            "range": "± 299697",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 869023,
            "range": "± 23986",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2548026,
            "range": "± 32999",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 28061191,
            "range": "± 840456",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4761335,
            "range": "± 69232",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2288,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 734776,
            "range": "± 49191",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42884614,
            "range": "± 157529",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46246087,
            "range": "± 2514668",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 62262857,
            "range": "± 4028947",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "1c92a7353e261f547501ad144ad64798d995ebb1",
          "message": "chore: bump versions for release -- iii(iii/v0.11.3-next.1)",
          "timestamp": "2026-04-22T13:33:55Z",
          "tree_id": "689d9321e4a93a477daf231d99a913fd6f064c0f",
          "url": "https://github.com/iii-hq/iii/commit/1c92a7353e261f547501ad144ad64798d995ebb1"
        },
        "date": 1776866537468,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3106,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 21190,
            "range": "± 262",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 90841,
            "range": "± 950",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 363973,
            "range": "± 3452",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 49357,
            "range": "± 1868",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 144320,
            "range": "± 1020",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86353,
            "range": "± 2383",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 490206,
            "range": "± 43645",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4393638,
            "range": "± 101642",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 870154,
            "range": "± 32684",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2538883,
            "range": "± 76586",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25586239,
            "range": "± 1210510",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4426777,
            "range": "± 26153",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2286,
            "range": "± 66",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 540235,
            "range": "± 25873",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42339147,
            "range": "± 237584",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 42891309,
            "range": "± 6128164",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 59653509,
            "range": "± 2836266",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 319743,
            "range": "± 10064",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 348048,
            "range": "± 4348",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 540386,
            "range": "± 21459",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2543208,
            "range": "± 119812",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 320284,
            "range": "± 10893",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1881,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2578,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9468,
            "range": "± 38",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86444,
            "range": "± 227",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1169,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 366,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 107,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1065,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 609,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 592,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1241,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 384,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1578,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6042,
            "range": "± 77",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 30875,
            "range": "± 96",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 39591,
            "range": "± 104",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 420,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 292,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 244,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 251,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 398,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 418,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 258,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 79,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 114,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 112,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 840,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 881,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1518,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1651,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 643,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 246,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 131,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 129,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1080,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1107,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1935,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2028,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 913,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 288,
            "range": "± 29",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33712,
            "range": "± 633",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52695,
            "range": "± 1058",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 100990,
            "range": "± 4944",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 328156,
            "range": "± 16404",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8555,
            "range": "± 256",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8599,
            "range": "± 131",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29248,
            "range": "± 395",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 113706,
            "range": "± 708",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 463209,
            "range": "± 14864",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7511,
            "range": "± 359",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8396,
            "range": "± 153",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17554,
            "range": "± 247",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123982,
            "range": "± 1195",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1438,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 390,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 131,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1433,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 440,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 39697,
            "range": "± 377",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 108,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 34244,
            "range": "± 544",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 56930,
            "range": "± 1052",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 128760,
            "range": "± 2367",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 464825,
            "range": "± 7397",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5375,
            "range": "± 18619",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18337,
            "range": "± 42887",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 64539,
            "range": "± 28399",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2262,
            "range": "± 186",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3285,
            "range": "± 164",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7157,
            "range": "± 322",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7989,
            "range": "± 776",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 29360,
            "range": "± 17752",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 99974,
            "range": "± 10668",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 669,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 823,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5865,
            "range": "± 58",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29826,
            "range": "± 1213",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 122937,
            "range": "± 515",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 676,
            "range": "± 38",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 171519,
            "range": "± 6110",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "fa74ed7e944965eeeb6247163e4b211ecd3129cc",
          "message": "chore: bump versions for release -- iii(iii/v0.11.3-next.2)",
          "timestamp": "2026-04-22T14:24:56Z",
          "tree_id": "c59fbf9372d5afb4ce8f4513b687a8d5fbf0e4e5",
          "url": "https://github.com/iii-hq/iii/commit/fa74ed7e944965eeeb6247163e4b211ecd3129cc"
        },
        "date": 1776869606358,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3109,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20830,
            "range": "± 514",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 91055,
            "range": "± 393",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 369641,
            "range": "± 811",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 48923,
            "range": "± 2571",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 143197,
            "range": "± 637",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88831,
            "range": "± 579",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 486034,
            "range": "± 734",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4451700,
            "range": "± 43158",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 869214,
            "range": "± 2303",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2525311,
            "range": "± 10696",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 26198659,
            "range": "± 860371",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4428172,
            "range": "± 36841",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2276,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 545679,
            "range": "± 4764",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42343076,
            "range": "± 219927",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46023732,
            "range": "± 3165630",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57982358,
            "range": "± 4907815",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 320887,
            "range": "± 10343",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 346661,
            "range": "± 3635",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 543273,
            "range": "± 14030",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2520954,
            "range": "± 115132",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 319556,
            "range": "± 10038",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1911,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2576,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9364,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86436,
            "range": "± 168",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1203,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 368,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 108,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1062,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 623,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 580,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1231,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 387,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1543,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6259,
            "range": "± 144",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 30360,
            "range": "± 61",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 39898,
            "range": "± 563",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 447,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 291,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 40,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 230,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 224,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 440,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 419,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 248,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 87,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 112,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 111,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 848,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 855,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1482,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1695,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 645,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 221,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 127,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 127,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1057,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1088,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1973,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2177,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 898,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 285,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33152,
            "range": "± 599",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52881,
            "range": "± 428",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 99715,
            "range": "± 5161",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 326335,
            "range": "± 23087",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8521,
            "range": "± 193",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8687,
            "range": "± 115",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29577,
            "range": "± 184",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 116827,
            "range": "± 577",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 474660,
            "range": "± 1898",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7594,
            "range": "± 63",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8560,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 19270,
            "range": "± 216",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123516,
            "range": "± 416",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1461,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 397,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 134,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1410,
            "range": "± 40",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 460,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 39695,
            "range": "± 227",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 108,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 33430,
            "range": "± 845",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 55955,
            "range": "± 407",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 122397,
            "range": "± 2576",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 471521,
            "range": "± 9916",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5384,
            "range": "± 514",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18239,
            "range": "± 442",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 63647,
            "range": "± 2681",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2227,
            "range": "± 148",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3263,
            "range": "± 120",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7176,
            "range": "± 157",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7911,
            "range": "± 160",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28701,
            "range": "± 604",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 97535,
            "range": "± 3326",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 666,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 806,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5861,
            "range": "± 333",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29561,
            "range": "± 722",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 122441,
            "range": "± 702",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 675,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 174004,
            "range": "± 6059",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "e021e0e1c1f14ec18bb93cf27fd336d20d9c5000",
          "message": "chore: bump versions for release -- iii(iii/v0.11.3)",
          "timestamp": "2026-04-22T14:35:08Z",
          "tree_id": "88f775a96e5783ced3c1cf8fc27669114e127b85",
          "url": "https://github.com/iii-hq/iii/commit/e021e0e1c1f14ec18bb93cf27fd336d20d9c5000"
        },
        "date": 1776871378010,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2763,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18855,
            "range": "± 402",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 81742,
            "range": "± 1580",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 326458,
            "range": "± 4625",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44270,
            "range": "± 1077",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133114,
            "range": "± 1592",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 85473,
            "range": "± 1382",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 462464,
            "range": "± 2691",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3890770,
            "range": "± 36755",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 859977,
            "range": "± 36493",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2390054,
            "range": "± 21710",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 23198753,
            "range": "± 683974",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4375862,
            "range": "± 57503",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2121,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 576419,
            "range": "± 9806",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42869584,
            "range": "± 133143",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 44427819,
            "range": "± 2454267",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 59980396,
            "range": "± 5736849",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 366916,
            "range": "± 18566",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 378987,
            "range": "± 17023",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 593205,
            "range": "± 17009",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2910097,
            "range": "± 277237",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 368259,
            "range": "± 20886",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1653,
            "range": "± 65",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2417,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8677,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 106555,
            "range": "± 3586",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1136,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 359,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1025,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 576,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 569,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1190,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 374,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1492,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 5881,
            "range": "± 32",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 29580,
            "range": "± 170",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37000,
            "range": "± 125",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 401,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 281,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 220,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 228,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 421,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 397,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 243,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 79,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 116,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 116,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 765,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 811,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1346,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1520,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 615,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 216,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 130,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 133,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 974,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1029,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1842,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1941,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 893,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 295,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 42066,
            "range": "± 1321",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59032,
            "range": "± 563",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 107054,
            "range": "± 4664",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 309325,
            "range": "± 14043",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8078,
            "range": "± 166",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8083,
            "range": "± 101",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27304,
            "range": "± 143",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 106930,
            "range": "± 567",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 433111,
            "range": "± 1362",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7007,
            "range": "± 99",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7881,
            "range": "± 58",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 52598,
            "range": "± 496",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 153669,
            "range": "± 3214",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1395,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 394,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 126,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1442,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 411,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38869,
            "range": "± 133",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 106,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 42054,
            "range": "± 1533",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 62736,
            "range": "± 414",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 128914,
            "range": "± 1346",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 435333,
            "range": "± 9988",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5457,
            "range": "± 1707",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18535,
            "range": "± 4362",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 61445,
            "range": "± 11768",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2400,
            "range": "± 456",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3173,
            "range": "± 288",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6986,
            "range": "± 359",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7710,
            "range": "± 918",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 27311,
            "range": "± 739",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 94637,
            "range": "± 3835",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 555,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 689,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4768,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24707,
            "range": "± 316",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101883,
            "range": "± 882",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 560,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 226021,
            "range": "± 6785",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "cd7eb05dc41244a0f015dde2935a6c59534521ff",
          "message": "chore: bump versions for release -- iii(iii/v0.11.4-next.1)",
          "timestamp": "2026-04-23T18:23:12Z",
          "tree_id": "491b0c4616c21edd1fe26d6e78c0da82186bf78f",
          "url": "https://github.com/iii-hq/iii/commit/cd7eb05dc41244a0f015dde2935a6c59534521ff"
        },
        "date": 1776970300024,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3051,
            "range": "± 29",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20969,
            "range": "± 374",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 90960,
            "range": "± 340",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 362749,
            "range": "± 2346",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 49352,
            "range": "± 2069",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 146460,
            "range": "± 463",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87538,
            "range": "± 1420",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 480916,
            "range": "± 1043",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4401827,
            "range": "± 17254",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 885121,
            "range": "± 7495",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2510952,
            "range": "± 13172",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 24324597,
            "range": "± 440178",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4508989,
            "range": "± 60698",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2277,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 546274,
            "range": "± 3776",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42175819,
            "range": "± 174730",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45031632,
            "range": "± 3217706",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57195588,
            "range": "± 1928173",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 318324,
            "range": "± 3497",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 346267,
            "range": "± 5752",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 539142,
            "range": "± 8346",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2407348,
            "range": "± 16105",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 320047,
            "range": "± 18380",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1885,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2610,
            "range": "± 46",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9310,
            "range": "± 49",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85985,
            "range": "± 145",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1170,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 352,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 106,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1055,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 581,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 584,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1210,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 385,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1591,
            "range": "± 59",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 5997,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 30678,
            "range": "± 150",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 39777,
            "range": "± 390",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 424,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 319,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 38,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 219,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 229,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 371,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 383,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 240,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 78,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 110,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 109,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 821,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 855,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1494,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1640,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 637,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 227,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 124,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 126,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1039,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1054,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1935,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2052,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 876,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 280,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33328,
            "range": "± 990",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52675,
            "range": "± 1556",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 100875,
            "range": "± 4463",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 321909,
            "range": "± 14633",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8719,
            "range": "± 172",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8891,
            "range": "± 135",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29996,
            "range": "± 209",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 115723,
            "range": "± 503",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 463920,
            "range": "± 3536",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7404,
            "range": "± 45",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8510,
            "range": "± 112",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17546,
            "range": "± 73",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123503,
            "range": "± 1517",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1437,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 390,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 132,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1495,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 434,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 40523,
            "range": "± 179",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 108,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 33471,
            "range": "± 492",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 56506,
            "range": "± 439",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 128824,
            "range": "± 3717",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 466581,
            "range": "± 24225",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5309,
            "range": "± 486",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18012,
            "range": "± 438",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 63547,
            "range": "± 2886",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2229,
            "range": "± 112",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3284,
            "range": "± 139",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7266,
            "range": "± 146",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7802,
            "range": "± 147",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28528,
            "range": "± 469",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 97407,
            "range": "± 3101",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 659,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 821,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5866,
            "range": "± 66",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29669,
            "range": "± 64",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 122798,
            "range": "± 684",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 666,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 170622,
            "range": "± 4265",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "46bcd418acca55cf76ef64c24926e7110749c6df",
          "message": "chore: bump versions for release -- iii(iii/v0.11.4-next.2)",
          "timestamp": "2026-04-24T01:12:52Z",
          "tree_id": "efc816f4de9e582fcaaa679f1daa2d33c239ee8e",
          "url": "https://github.com/iii-hq/iii/commit/46bcd418acca55cf76ef64c24926e7110749c6df"
        },
        "date": 1776993973264,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2709,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18476,
            "range": "± 356",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 82556,
            "range": "± 1038",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 328114,
            "range": "± 2103",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44576,
            "range": "± 924",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133158,
            "range": "± 1971",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 85706,
            "range": "± 1464",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 464733,
            "range": "± 1271",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3760794,
            "range": "± 31439",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 861186,
            "range": "± 2144",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2387399,
            "range": "± 21559",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21275110,
            "range": "± 517840",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4322992,
            "range": "± 8725",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2086,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 557367,
            "range": "± 15928",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42141331,
            "range": "± 131812",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45768689,
            "range": "± 2302109",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58599777,
            "range": "± 4779368",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "373b5561c3ccac95d9628fbbadde414a26cf5d17",
          "message": "chore: bump versions for release -- iii(iii/v0.11.4-next.3)",
          "timestamp": "2026-04-24T23:04:31Z",
          "tree_id": "f2f757fb1a4a3a8cb81eb12491c06ef54780c76c",
          "url": "https://github.com/iii-hq/iii/commit/373b5561c3ccac95d9628fbbadde414a26cf5d17"
        },
        "date": 1777073587447,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2796,
            "range": "± 35",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19209,
            "range": "± 901",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 85691,
            "range": "± 267",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 332591,
            "range": "± 1930",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44217,
            "range": "± 1055",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 125155,
            "range": "± 495",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 84736,
            "range": "± 1945",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 470223,
            "range": "± 1422",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3901411,
            "range": "± 60696",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 860871,
            "range": "± 5208",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2440870,
            "range": "± 28954",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 23412728,
            "range": "± 746463",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4342287,
            "range": "± 49725",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2173,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 576560,
            "range": "± 11686",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42830810,
            "range": "± 189229",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46835148,
            "range": "± 2229985",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58155517,
            "range": "± 2292830",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 372306,
            "range": "± 15310",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 377456,
            "range": "± 8172",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 594194,
            "range": "± 21692",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2518496,
            "range": "± 135861",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 370553,
            "range": "± 15425",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1717,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2319,
            "range": "± 69",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9120,
            "range": "± 111",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86559,
            "range": "± 636",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1135,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 351,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 105,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 972,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 631,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 574,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1313,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 400,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1555,
            "range": "± 127",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6100,
            "range": "± 99",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 31343,
            "range": "± 119",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38114,
            "range": "± 880",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 402,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 284,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 223,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 231,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 392,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 392,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 252,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 79,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 107,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 106,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 802,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 801,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1400,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1517,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 608,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 220,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 124,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 125,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1034,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1054,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1843,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1934,
            "range": "± 41",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 865,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 278,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 42106,
            "range": "± 1491",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59438,
            "range": "± 647",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 108747,
            "range": "± 3991",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 308440,
            "range": "± 10023",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8213,
            "range": "± 168",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8174,
            "range": "± 134",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27820,
            "range": "± 141",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108999,
            "range": "± 2185",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 441563,
            "range": "± 8415",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6973,
            "range": "± 80",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7979,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 49879,
            "range": "± 804",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 124675,
            "range": "± 958",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1358,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 388,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 126,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1437,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 438,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38355,
            "range": "± 192",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 103,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 42249,
            "range": "± 2432",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 64930,
            "range": "± 489",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 131578,
            "range": "± 1865",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 441807,
            "range": "± 7152",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5280,
            "range": "± 1046",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16721,
            "range": "± 982",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57390,
            "range": "± 2666",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2222,
            "range": "± 261",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3162,
            "range": "± 219",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6893,
            "range": "± 159",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7494,
            "range": "± 513",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 27101,
            "range": "± 358",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 95551,
            "range": "± 2810",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 556,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 693,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4834,
            "range": "± 61",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24722,
            "range": "± 556",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 100941,
            "range": "± 486",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 558,
            "range": "± 9",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "76a4517e6c498eb55b9217a5e060c678adcd3c59",
          "message": "chore: bump versions for release -- iii(iii/v0.11.4-next.4)",
          "timestamp": "2026-04-27T15:24:42Z",
          "tree_id": "db989d722281a3ec4159082da6987717d65781ba",
          "url": "https://github.com/iii-hq/iii/commit/76a4517e6c498eb55b9217a5e060c678adcd3c59"
        },
        "date": 1777304300907,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2759,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19060,
            "range": "± 387",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 83502,
            "range": "± 297",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 334160,
            "range": "± 2043",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 47088,
            "range": "± 1790",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 132214,
            "range": "± 811",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86674,
            "range": "± 1056",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 478292,
            "range": "± 3928",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3877110,
            "range": "± 55957",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 879691,
            "range": "± 4446",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2475237,
            "range": "± 15754",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 23287312,
            "range": "± 591470",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4452651,
            "range": "± 49678",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2172,
            "range": "± 39",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 584089,
            "range": "± 8487",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42824814,
            "range": "± 158382",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46530032,
            "range": "± 1812075",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58861755,
            "range": "± 4459633",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 367967,
            "range": "± 20887",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "22ec83cee9ee1539a3b44a355c500d97c7b4c5e9",
          "message": "chore: bump versions for release -- iii(iii/v0.11.4-next.5)",
          "timestamp": "2026-04-28T16:03:03Z",
          "tree_id": "3be04a169258d34b97d46da5e26a459f14b999e1",
          "url": "https://github.com/iii-hq/iii/commit/22ec83cee9ee1539a3b44a355c500d97c7b4c5e9"
        },
        "date": 1777392977017,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2686,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18749,
            "range": "± 549",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 82325,
            "range": "± 378",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 326878,
            "range": "± 1778",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 46367,
            "range": "± 1733",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133100,
            "range": "± 583",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87380,
            "range": "± 547",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 466496,
            "range": "± 2036",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3882057,
            "range": "± 110286",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 869866,
            "range": "± 4482",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2439966,
            "range": "± 41723",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 23591200,
            "range": "± 922905",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4384199,
            "range": "± 24353",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2137,
            "range": "± 39",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 591848,
            "range": "± 8796",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "e6dd7f7f71131fa090ce61505df76d0afbeae40a",
          "message": "chore: bump versions for release -- iii(iii/v0.11.4)",
          "timestamp": "2026-04-30T11:22:50Z",
          "tree_id": "598e31c2ecdbf6d6d3b02b1424679e651f7eb1f1",
          "url": "https://github.com/iii-hq/iii/commit/e6dd7f7f71131fa090ce61505df76d0afbeae40a"
        },
        "date": 1777549874257,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2776,
            "range": "± 50",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19090,
            "range": "± 335",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 84246,
            "range": "± 181",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 332435,
            "range": "± 1633",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45999,
            "range": "± 1361",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 132007,
            "range": "± 1094",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88086,
            "range": "± 1239",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 471721,
            "range": "± 1993",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3740980,
            "range": "± 30835",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 870135,
            "range": "± 4460",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2416207,
            "range": "± 22680",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 20795022,
            "range": "± 374727",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4368011,
            "range": "± 13939",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2071,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 574913,
            "range": "± 14634",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42071645,
            "range": "± 54448",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45980564,
            "range": "± 2235677",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58567183,
            "range": "± 3906703",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 327773,
            "range": "± 11574",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 349700,
            "range": "± 9282",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 574963,
            "range": "± 17885",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2333592,
            "range": "± 118675",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 336860,
            "range": "± 14632",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1645,
            "range": "± 46",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2258,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8941,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85532,
            "range": "± 495",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1182,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 368,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 983,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 669,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 601,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1424,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 410,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1624,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6465,
            "range": "± 42",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32058,
            "range": "± 146",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38759,
            "range": "± 115",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 403,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 282,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 217,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 221,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 383,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 391,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 237,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 78,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 790,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 789,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1373,
            "range": "± 38",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1520,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 613,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 220,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 130,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 130,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1032,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1022,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1803,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2033,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 916,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 272,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 38782,
            "range": "± 2027",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59516,
            "range": "± 638",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 103713,
            "range": "± 3798",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 299009,
            "range": "± 9748",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8259,
            "range": "± 349",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8105,
            "range": "± 107",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27506,
            "range": "± 197",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108125,
            "range": "± 524",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 436196,
            "range": "± 2258",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7100,
            "range": "± 64",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8035,
            "range": "± 79",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 51974,
            "range": "± 430",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 151275,
            "range": "± 1215",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1384,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 396,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 125,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1449,
            "range": "± 42",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 445,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37168,
            "range": "± 370",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 107,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 38930,
            "range": "± 3253",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 57669,
            "range": "± 692",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 125038,
            "range": "± 1429",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 421674,
            "range": "± 7065",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4917,
            "range": "± 410",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16387,
            "range": "± 210",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57894,
            "range": "± 2160",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2089,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3316,
            "range": "± 116",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7032,
            "range": "± 204",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7208,
            "range": "± 92",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26792,
            "range": "± 331",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 93064,
            "range": "± 2147",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 543,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 697,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4867,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 26589,
            "range": "± 153",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 102020,
            "range": "± 966",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 543,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 217732,
            "range": "± 4862",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "a28591777f966f99a0a19c1ba28411d592995730",
          "message": "chore: bump versions for release -- iii(iii/v0.11.5-next.1)",
          "timestamp": "2026-04-30T16:37:46Z",
          "tree_id": "8341ac50d6c502f37a77705a28eefaf5170d6a7f",
          "url": "https://github.com/iii-hq/iii/commit/a28591777f966f99a0a19c1ba28411d592995730"
        },
        "date": 1777567821768,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2561,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 16765,
            "range": "± 243",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 74723,
            "range": "± 1037",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 292352,
            "range": "± 2526",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 49527,
            "range": "± 1525",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 145837,
            "range": "± 463",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 92819,
            "range": "± 1630",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 488641,
            "range": "± 1075",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 2959042,
            "range": "± 28240",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 905599,
            "range": "± 3113",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2578495,
            "range": "± 32633",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 17281497,
            "range": "± 619182",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4664804,
            "range": "± 7976",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 1961,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 572976,
            "range": "± 7655",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "committer": {
            "email": "iii-ci[bot]@users.noreply.github.com",
            "name": "iii-ci[bot]"
          },
          "distinct": true,
          "id": "421a9e813bfcaf02aab5a50600234cacc106bb1c",
          "message": "chore: bump versions for release -- iii(iii/v0.11.5)",
          "timestamp": "2026-04-30T17:04:10Z",
          "tree_id": "46c05dafd24bee87eaf427ba27616399d862a445",
          "url": "https://github.com/iii-hq/iii/commit/421a9e813bfcaf02aab5a50600234cacc106bb1c"
        },
        "date": 1777570367850,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2694,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18564,
            "range": "± 571",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 83543,
            "range": "± 345",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 326050,
            "range": "± 927",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45475,
            "range": "± 253",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 131506,
            "range": "± 834",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87008,
            "range": "± 649",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 457103,
            "range": "± 1838",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3748614,
            "range": "± 65768",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 853052,
            "range": "± 1904",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2359249,
            "range": "± 13320",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21296638,
            "range": "± 1233263",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4317077,
            "range": "± 59156",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2102,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 542565,
            "range": "± 8357",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42366408,
            "range": "± 270340",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 44797239,
            "range": "± 6333838",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 59492586,
            "range": "± 2915197",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 332877,
            "range": "± 23565",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 344117,
            "range": "± 6371",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 547058,
            "range": "± 21569",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2325143,
            "range": "± 141852",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 331794,
            "range": "± 20952",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1674,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2308,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8884,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86012,
            "range": "± 221",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1142,
            "range": "± 80",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 366,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 104,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 991,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 671,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 635,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1390,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 406,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1592,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6276,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 31751,
            "range": "± 449",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 39277,
            "range": "± 229",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 410,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 284,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 255,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 238,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 455,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 481,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 296,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 91,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 114,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 115,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 758,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 795,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1385,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1494,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 612,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 217,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 131,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 132,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1036,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1065,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1880,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2047,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 918,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 283,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 37139,
            "range": "± 1461",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 57383,
            "range": "± 492",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 101503,
            "range": "± 4345",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 298619,
            "range": "± 9221",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8144,
            "range": "± 188",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8121,
            "range": "± 129",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27486,
            "range": "± 212",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 106167,
            "range": "± 541",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 435263,
            "range": "± 2300",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7035,
            "range": "± 65",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8039,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50698,
            "range": "± 319",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123671,
            "range": "± 566",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1408,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 384,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 125,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1466,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 441,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37526,
            "range": "± 212",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 37196,
            "range": "± 2064",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 57242,
            "range": "± 773",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 121127,
            "range": "± 2589",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 415211,
            "range": "± 6765",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5028,
            "range": "± 1290",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16281,
            "range": "± 290",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57219,
            "range": "± 2165",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2180,
            "range": "± 140",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3107,
            "range": "± 294",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6852,
            "range": "± 103",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7287,
            "range": "± 151",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26999,
            "range": "± 361",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 94326,
            "range": "± 7858",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 548,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 719,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4853,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 25452,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 104367,
            "range": "± 1462",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 551,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 211251,
            "range": "± 7793",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}