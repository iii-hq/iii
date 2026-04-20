window.BENCHMARK_DATA = {
  "lastUpdate": 1776691418665,
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
      }
    ]
  }
}