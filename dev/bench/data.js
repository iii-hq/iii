window.BENCHMARK_DATA = {
  "lastUpdate": 1782313184511,
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
          "id": "7762cdbee9e882b9160adf0ec71f42f4ceedf9e8",
          "message": "chore: bump versions for release -- iii(iii/v0.11.6-next.1)",
          "timestamp": "2026-05-04T12:21:38Z",
          "tree_id": "935cfcd281fde2d31dac3c9e8b8373ef9d8906df",
          "url": "https://github.com/iii-hq/iii/commit/7762cdbee9e882b9160adf0ec71f42f4ceedf9e8"
        },
        "date": 1777899014044,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3003,
            "range": "± 103",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20803,
            "range": "± 912",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 91074,
            "range": "± 938",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 364588,
            "range": "± 9906",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 47383,
            "range": "± 1509",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 141898,
            "range": "± 1290",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86940,
            "range": "± 3375",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 490489,
            "range": "± 2678",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4987800,
            "range": "± 187873",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 869760,
            "range": "± 24324",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2579196,
            "range": "± 47593",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 27528658,
            "range": "± 684155",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4483188,
            "range": "± 20353",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2310,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 538871,
            "range": "± 23927",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42584869,
            "range": "± 238840",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45461496,
            "range": "± 3453699",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58260561,
            "range": "± 2803803",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 318870,
            "range": "± 11022",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 345454,
            "range": "± 3350",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 545933,
            "range": "± 7104",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2662569,
            "range": "± 134222",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 320392,
            "range": "± 3167",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1852,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2563,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9328,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86387,
            "range": "± 2197",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1210,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 393,
            "range": "± 1",
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
            "value": 1034,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 726,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 649,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1461,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 447,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1753,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6901,
            "range": "± 371",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34441,
            "range": "± 265",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 41681,
            "range": "± 471",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 423,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 295,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 39,
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
            "value": 218,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 220,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 375,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 386,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 239,
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
            "value": 110,
            "range": "± 0",
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
            "value": 867,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 879,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1509,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1646,
            "range": "± 46",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 666,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 238,
            "range": "± 4",
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
            "value": 1050,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1065,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1914,
            "range": "± 41",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2093,
            "range": "± 62",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 898,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 279,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33728,
            "range": "± 663",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52482,
            "range": "± 383",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 101531,
            "range": "± 5449",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 321954,
            "range": "± 16442",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8708,
            "range": "± 408",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8517,
            "range": "± 194",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29275,
            "range": "± 462",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 115582,
            "range": "± 1799",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 466591,
            "range": "± 1747",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7436,
            "range": "± 45",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8404,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17845,
            "range": "± 74",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123935,
            "range": "± 5907",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1427,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 428,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 132,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1502,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 483,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 39226,
            "range": "± 82",
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
            "value": 33662,
            "range": "± 578",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 56389,
            "range": "± 1296",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 127358,
            "range": "± 3688",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 462093,
            "range": "± 7146",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5378,
            "range": "± 520",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18218,
            "range": "± 741",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 63710,
            "range": "± 2360",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2213,
            "range": "± 234",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3335,
            "range": "± 164",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7517,
            "range": "± 184",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7852,
            "range": "± 332",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28946,
            "range": "± 450",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 99069,
            "range": "± 3368",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 665,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 801,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5847,
            "range": "± 29",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29744,
            "range": "± 430",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 122282,
            "range": "± 1341",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 664,
            "range": "± 33",
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
          "id": "a67d29ac364c400215e12e2910af476dc729c229",
          "message": "chore: bump versions for release -- iii(iii/v0.11.6-next.2)",
          "timestamp": "2026-05-04T14:34:51Z",
          "tree_id": "6ce7d9d24e23e9c4e3ed8a6652efeb26cc53413f",
          "url": "https://github.com/iii-hq/iii/commit/a67d29ac364c400215e12e2910af476dc729c229"
        },
        "date": 1777907009536,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2737,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18980,
            "range": "± 538",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 83987,
            "range": "± 429",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 335012,
            "range": "± 3329",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44582,
            "range": "± 1062",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 131441,
            "range": "± 787",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86091,
            "range": "± 1969",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 461378,
            "range": "± 3282",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3807566,
            "range": "± 19959",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 852632,
            "range": "± 5541",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2356382,
            "range": "± 14381",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21416242,
            "range": "± 696487",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4279716,
            "range": "± 17092",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2168,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 587935,
            "range": "± 14928",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42496810,
            "range": "± 237239",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46545795,
            "range": "± 7189494",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57762434,
            "range": "± 7449001",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 363242,
            "range": "± 19934",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 380795,
            "range": "± 12958",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 571193,
            "range": "± 15975",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2444912,
            "range": "± 146808",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 370928,
            "range": "± 17816",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1726,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2305,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8476,
            "range": "± 50",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85911,
            "range": "± 426",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1104,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 364,
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
            "value": 964,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 664,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 598,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1425,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 415,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1646,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6422,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 31444,
            "range": "± 498",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38490,
            "range": "± 219",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 417,
            "range": "± 4",
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
            "value": 39,
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
            "value": 268,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 301,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 454,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 448,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 294,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 93,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 105,
            "range": "± 0",
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
            "value": 814,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 778,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1367,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1481,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 603,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 216,
            "range": "± 0",
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
            "value": 125,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1030,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1067,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1889,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1999,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 928,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 282,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41793,
            "range": "± 1145",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58799,
            "range": "± 1275",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 107489,
            "range": "± 4900",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 307924,
            "range": "± 12039",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8344,
            "range": "± 152",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8131,
            "range": "± 176",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27925,
            "range": "± 176",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108326,
            "range": "± 337",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 441586,
            "range": "± 2090",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7130,
            "range": "± 81",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8471,
            "range": "± 90",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50941,
            "range": "± 387",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123565,
            "range": "± 899",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1382,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 395,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 128,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1447,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 438,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37923,
            "range": "± 235",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 101,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 43051,
            "range": "± 2340",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 65386,
            "range": "± 959",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 124819,
            "range": "± 3265",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 441055,
            "range": "± 10739",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5453,
            "range": "± 1039",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16541,
            "range": "± 817",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56752,
            "range": "± 3710",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2198,
            "range": "± 183",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3132,
            "range": "± 109",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6889,
            "range": "± 256",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7454,
            "range": "± 38522",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26876,
            "range": "± 456",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 93633,
            "range": "± 3098",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 549,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 703,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4863,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24991,
            "range": "± 53",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 102195,
            "range": "± 814",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 556,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 229996,
            "range": "± 5122",
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
          "id": "7989b17f05432a6dc22f8e549170b32f6a8227fb",
          "message": "chore: bump versions for release -- iii(iii/v0.11.6-next.3)",
          "timestamp": "2026-05-05T12:12:52Z",
          "tree_id": "7a270a15fa75dba19aee1675f78e29c56d9df086",
          "url": "https://github.com/iii-hq/iii/commit/7989b17f05432a6dc22f8e549170b32f6a8227fb"
        },
        "date": 1777984886579,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2986,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20861,
            "range": "± 131",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 89410,
            "range": "± 960",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 360385,
            "range": "± 3722",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 47677,
            "range": "± 1358",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 140561,
            "range": "± 1495",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88363,
            "range": "± 1198",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 483055,
            "range": "± 5123",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4485689,
            "range": "± 69215",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 868239,
            "range": "± 5308",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2526622,
            "range": "± 23920",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25296946,
            "range": "± 753608",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4462556,
            "range": "± 26904",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2342,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 545006,
            "range": "± 4212",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42164047,
            "range": "± 293520",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 44712667,
            "range": "± 2883531",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58850630,
            "range": "± 4563294",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 318966,
            "range": "± 4640",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 345802,
            "range": "± 5560",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 535328,
            "range": "± 12401",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2595757,
            "range": "± 105820",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 318375,
            "range": "± 11754",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1887,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2531,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9295,
            "range": "± 162",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86068,
            "range": "± 167",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1201,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 396,
            "range": "± 6",
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
            "value": 1037,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 689,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 650,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1392,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 457,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1732,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6926,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34529,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 41669,
            "range": "± 278",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 372,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 291,
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
            "value": 236,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 373,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 386,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 242,
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
            "value": 111,
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
            "value": 849,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 867,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1517,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1625,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 642,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 231,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 139,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 139,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1059,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1087,
            "range": "± 18",
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
            "value": 2083,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 858,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 291,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33840,
            "range": "± 811",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52184,
            "range": "± 589",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 100078,
            "range": "± 5470",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 324175,
            "range": "± 13045",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8843,
            "range": "± 187",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8813,
            "range": "± 85",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29763,
            "range": "± 159",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 115083,
            "range": "± 395",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 464698,
            "range": "± 3606",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7456,
            "range": "± 72",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8422,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17507,
            "range": "± 90",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123959,
            "range": "± 448",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1500,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 397,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 135,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1430,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 469,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 39466,
            "range": "± 391",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 112,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 33948,
            "range": "± 815",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 56232,
            "range": "± 517",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 127533,
            "range": "± 1971",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 459735,
            "range": "± 8020",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5579,
            "range": "± 720",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18338,
            "range": "± 544",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 63772,
            "range": "± 3488",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2333,
            "range": "± 141",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3403,
            "range": "± 139",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7527,
            "range": "± 156",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7980,
            "range": "± 261",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 29453,
            "range": "± 1184",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 98104,
            "range": "± 3651",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 660,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 824,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5858,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29555,
            "range": "± 159",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 122283,
            "range": "± 1176",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 670,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 169430,
            "range": "± 4915",
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
          "id": "49a098612532eee0525b4bd179d29f8d1dfeac1c",
          "message": "chore: bump versions for release -- iii(iii/v0.11.6-next.4)",
          "timestamp": "2026-05-05T14:00:12Z",
          "tree_id": "2d327b1c4918f8d2b6b2900e74121e2c02121fe2",
          "url": "https://github.com/iii-hq/iii/commit/49a098612532eee0525b4bd179d29f8d1dfeac1c"
        },
        "date": 1777991328079,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2975,
            "range": "± 32",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20821,
            "range": "± 103",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 89991,
            "range": "± 1779",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 358708,
            "range": "± 1380",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 48231,
            "range": "± 1449",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 138274,
            "range": "± 399",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 90054,
            "range": "± 1232",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 493425,
            "range": "± 836",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4448558,
            "range": "± 22818",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 905845,
            "range": "± 2406",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2579628,
            "range": "± 16899",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 24610409,
            "range": "± 930325",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4617142,
            "range": "± 74719",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2384,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 548826,
            "range": "± 27530",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42279155,
            "range": "± 168460",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 43473698,
            "range": "± 2888792",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58586188,
            "range": "± 3440869",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 314976,
            "range": "± 10704",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 341220,
            "range": "± 8488",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 541886,
            "range": "± 8272",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2592024,
            "range": "± 103798",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 318617,
            "range": "± 17786",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1879,
            "range": "± 70",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2497,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9374,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86654,
            "range": "± 324",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1229,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 396,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 107,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1035,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 668,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 636,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1393,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 446,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1748,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6995,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34669,
            "range": "± 187",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 41009,
            "range": "± 87",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 426,
            "range": "± 8",
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
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 209,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 261,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 414,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 415,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 273,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 86,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 110,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 110,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 880,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 874,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1489,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1610,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 667,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 226,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 126,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 128,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1088,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1118,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2001,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2136,
            "range": "± 56",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 925,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 292,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33610,
            "range": "± 1417",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52391,
            "range": "± 1558",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 101618,
            "range": "± 6137",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 323400,
            "range": "± 14690",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8646,
            "range": "± 163",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8704,
            "range": "± 118",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29301,
            "range": "± 160",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 115753,
            "range": "± 554",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 466088,
            "range": "± 7644",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7400,
            "range": "± 122",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8437,
            "range": "± 195",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17486,
            "range": "± 116",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123141,
            "range": "± 307",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1456,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 402,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 144,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1622,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 471,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 40395,
            "range": "± 128",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 112,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 33838,
            "range": "± 579",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 56533,
            "range": "± 813",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 127161,
            "range": "± 1336",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 451924,
            "range": "± 8720",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5360,
            "range": "± 545",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18714,
            "range": "± 1124",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 63908,
            "range": "± 2826",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2224,
            "range": "± 122",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3314,
            "range": "± 322",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7462,
            "range": "± 123",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7769,
            "range": "± 41233",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28323,
            "range": "± 376",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 97543,
            "range": "± 3236",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 664,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 798,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5857,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29905,
            "range": "± 525",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 123143,
            "range": "± 826",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 670,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 170143,
            "range": "± 10087",
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
          "id": "6a29cac8ea6501a28ee56dce267211f39ac0e52c",
          "message": "chore: bump versions for release -- iii(iii/v0.11.6-next.5)",
          "timestamp": "2026-05-05T14:05:13Z",
          "tree_id": "ea55cf33e5d0d5bd181cf8ea22e8e63f7a72d557",
          "url": "https://github.com/iii-hq/iii/commit/6a29cac8ea6501a28ee56dce267211f39ac0e52c"
        },
        "date": 1777993048851,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2706,
            "range": "± 48",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19078,
            "range": "± 584",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 82644,
            "range": "± 1518",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 332899,
            "range": "± 3332",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45974,
            "range": "± 1858",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 131423,
            "range": "± 2392",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87300,
            "range": "± 1878",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 465296,
            "range": "± 11932",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3810748,
            "range": "± 60168",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 892701,
            "range": "± 12405",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2372184,
            "range": "± 21663",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 20745380,
            "range": "± 575582",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4499770,
            "range": "± 9714",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2149,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 567774,
            "range": "± 16094",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42009421,
            "range": "± 142383",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 43522248,
            "range": "± 5802370",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56472726,
            "range": "± 3878098",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 340298,
            "range": "± 17995",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 371860,
            "range": "± 17331",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 567965,
            "range": "± 23244",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2310915,
            "range": "± 132121",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 346467,
            "range": "± 14470",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1713,
            "range": "± 35",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2314,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8633,
            "range": "± 188",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85416,
            "range": "± 2595",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1128,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 370,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 104,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 970,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 655,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 600,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1382,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 405,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1622,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6099,
            "range": "± 335",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 31077,
            "range": "± 909",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38276,
            "range": "± 780",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 414,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 278,
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
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 268,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 262,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 465,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 474,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 295,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 100,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 102,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 101,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 728,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 788,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1335,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1471,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 594,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 211,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 128,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 132,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1091,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1073,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1860,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2078,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 923,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 290,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41538,
            "range": "± 1288",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59182,
            "range": "± 750",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 107952,
            "range": "± 6671",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 310844,
            "range": "± 11665",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8265,
            "range": "± 159",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8103,
            "range": "± 136",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27582,
            "range": "± 269",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108130,
            "range": "± 2170",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 441601,
            "range": "± 3576",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6946,
            "range": "± 183",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8277,
            "range": "± 159",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50460,
            "range": "± 719",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123114,
            "range": "± 1421",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1391,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 403,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 129,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1409,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 436,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38020,
            "range": "± 217",
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
            "value": 41399,
            "range": "± 1959",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 64080,
            "range": "± 875",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 122472,
            "range": "± 2082",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 427162,
            "range": "± 7552",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4829,
            "range": "± 429",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 15922,
            "range": "± 992",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 55414,
            "range": "± 2404",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2066,
            "range": "± 59",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3077,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6812,
            "range": "± 140",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7119,
            "range": "± 223",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26207,
            "range": "± 766",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 92044,
            "range": "± 3577",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 545,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 704,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4832,
            "range": "± 102",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24856,
            "range": "± 1062",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101379,
            "range": "± 1829",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 547,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 226515,
            "range": "± 4111",
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
          "id": "655293c3ceca87ffddcda06b3df28104c2d15f4b",
          "message": "chore: bump versions for release -- iii(iii/v0.11.6-next.6)",
          "timestamp": "2026-05-05T17:46:25Z",
          "tree_id": "2ee37182a9b9829cec5ff45c63ea2e16ce6acd5a",
          "url": "https://github.com/iii-hq/iii/commit/655293c3ceca87ffddcda06b3df28104c2d15f4b"
        },
        "date": 1778003991851,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2771,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19624,
            "range": "± 609",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 83171,
            "range": "± 184",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 335821,
            "range": "± 4658",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45784,
            "range": "± 1733",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133511,
            "range": "± 495",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86591,
            "range": "± 1914",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 462015,
            "range": "± 2085",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3795360,
            "range": "± 28648",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 861887,
            "range": "± 15480",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2368286,
            "range": "± 28200",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 22846650,
            "range": "± 583928",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4340441,
            "range": "± 72913",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2154,
            "range": "± 35",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 587636,
            "range": "± 16915",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42595016,
            "range": "± 289268",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46065325,
            "range": "± 2153346",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 59005162,
            "range": "± 10664127",
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
          "id": "26ee6b0d05002e383072d6ad03af1e311984eb42",
          "message": "chore: bump versions for release -- iii(iii/v0.11.6)",
          "timestamp": "2026-05-05T23:08:00Z",
          "tree_id": "70dc056ffa9343438b9362383816d21e0717879f",
          "url": "https://github.com/iii-hq/iii/commit/26ee6b0d05002e383072d6ad03af1e311984eb42"
        },
        "date": 1778024190664,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2679,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18486,
            "range": "± 640",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 80094,
            "range": "± 293",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 324113,
            "range": "± 1248",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45975,
            "range": "± 1633",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 132911,
            "range": "± 794",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 84994,
            "range": "± 1447",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 466845,
            "range": "± 4728",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3731746,
            "range": "± 23746",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 856668,
            "range": "± 2389",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2384001,
            "range": "± 17252",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21329797,
            "range": "± 418165",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4285307,
            "range": "± 16366",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2082,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 567332,
            "range": "± 12215",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42057248,
            "range": "± 130299",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45655509,
            "range": "± 1320828",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 55278771,
            "range": "± 4833919",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 350905,
            "range": "± 14626",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 364153,
            "range": "± 14468",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 564815,
            "range": "± 17530",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2293652,
            "range": "± 77314",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 344814,
            "range": "± 15279",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1793,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2448,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 10436,
            "range": "± 229",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85690,
            "range": "± 371",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1161,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 379,
            "range": "± 19",
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
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 663,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 617,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1386,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 409,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1591,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6593,
            "range": "± 29",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32785,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38860,
            "range": "± 220",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 426,
            "range": "± 4",
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
            "value": 263,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 265,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 450,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 468,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 278,
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
            "value": 750,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 802,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1369,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1499,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 621,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 215,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 129,
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
            "value": 1051,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1083,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1880,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2018,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 910,
            "range": "± 5",
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
            "value": 40386,
            "range": "± 852",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 57911,
            "range": "± 537",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 105758,
            "range": "± 4312",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 305563,
            "range": "± 12173",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8167,
            "range": "± 210",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8137,
            "range": "± 137",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27748,
            "range": "± 143",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108471,
            "range": "± 409",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 437584,
            "range": "± 1888",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6864,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7945,
            "range": "± 72",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 51736,
            "range": "± 301",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 122465,
            "range": "± 647",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1424,
            "range": "± 60",
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
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1404,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 446,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38057,
            "range": "± 236",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 105,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 40880,
            "range": "± 1835",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 62758,
            "range": "± 411",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 127323,
            "range": "± 2619",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 427757,
            "range": "± 5451",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4925,
            "range": "± 432",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16203,
            "range": "± 278",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57727,
            "range": "± 2250",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2155,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3092,
            "range": "± 77",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6798,
            "range": "± 104",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7307,
            "range": "± 67",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 27095,
            "range": "± 256",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 95519,
            "range": "± 3062",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 555,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 693,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4869,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24873,
            "range": "± 354",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101211,
            "range": "± 621",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 560,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 223103,
            "range": "± 5658",
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
          "id": "1cdc9d4a4544ced6c0c0bedf2448a45db671769f",
          "message": "chore: bump versions for release -- iii(iii/v0.11.7-next.1)",
          "timestamp": "2026-05-07T12:22:23Z",
          "tree_id": "c4092ccc97cc9367eda5dc2cc6ba07b410157588",
          "url": "https://github.com/iii-hq/iii/commit/1cdc9d4a4544ced6c0c0bedf2448a45db671769f"
        },
        "date": 1778157334906,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2700,
            "range": "± 41",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18832,
            "range": "± 260",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 81402,
            "range": "± 387",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 325446,
            "range": "± 4348",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44621,
            "range": "± 1102",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133155,
            "range": "± 730",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 89840,
            "range": "± 1546",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 464017,
            "range": "± 7500",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4254858,
            "range": "± 92460",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 892112,
            "range": "± 3610",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2426157,
            "range": "± 54683",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 22331506,
            "range": "± 865544",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4472320,
            "range": "± 20216",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2131,
            "range": "± 18",
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
          "id": "bad3f7d2359da126ad40cbda067c429a8555b43c",
          "message": "chore: bump versions for release -- iii(iii/v0.11.7-next.2)",
          "timestamp": "2026-05-14T12:12:29Z",
          "tree_id": "33a2cbaeaf79b9a0ee5a6f1acc903fd4ce7004fe",
          "url": "https://github.com/iii-hq/iii/commit/bad3f7d2359da126ad40cbda067c429a8555b43c"
        },
        "date": 1778762451127,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2527,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 16575,
            "range": "± 204",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 75749,
            "range": "± 1058",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 300689,
            "range": "± 1283",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 48940,
            "range": "± 1679",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 150267,
            "range": "± 681",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 92112,
            "range": "± 1559",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 482588,
            "range": "± 864",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3099575,
            "range": "± 65891",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 914392,
            "range": "± 2169",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2562374,
            "range": "± 13131",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 20487419,
            "range": "± 529224",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4735528,
            "range": "± 29352",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2045,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 536475,
            "range": "± 9715",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42830195,
            "range": "± 137343",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 48185070,
            "range": "± 1012257",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 59378898,
            "range": "± 5110123",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 267804,
            "range": "± 14524",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 299357,
            "range": "± 10145",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 550568,
            "range": "± 14623",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 3557335,
            "range": "± 262693",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 267257,
            "range": "± 13734",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1363,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 1761,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8381,
            "range": "± 62",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 203897,
            "range": "± 416",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1116,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 433,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 108,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1071,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 686,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 637,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1467,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 388,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1531,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6077,
            "range": "± 61",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32795,
            "range": "± 70",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 41272,
            "range": "± 124",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 533,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 390,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 35,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 35,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 234,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 225,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 414,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 441,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 256,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 82,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 91,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 91,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 668,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 670,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1296,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1352,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 524,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 183,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 100,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 99,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 944,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 964,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1782,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1828,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 826,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 253,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 31401,
            "range": "± 1295",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 47787,
            "range": "± 384",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 101473,
            "range": "± 1701",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 368154,
            "range": "± 10723",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8056,
            "range": "± 130",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8093,
            "range": "± 117",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27913,
            "range": "± 139",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 109573,
            "range": "± 1224",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 432496,
            "range": "± 1818",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7000,
            "range": "± 170",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7947,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 16541,
            "range": "± 44",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 294482,
            "range": "± 661",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1502,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 464,
            "range": "± 5",
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
            "value": 1493,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 422,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 40139,
            "range": "± 119",
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
            "value": 31646,
            "range": "± 1368",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 53823,
            "range": "± 418",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 142027,
            "range": "± 2875",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 537857,
            "range": "± 14184",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 6675,
            "range": "± 2448",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 19525,
            "range": "± 2823",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 66290,
            "range": "± 4350",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 3172,
            "range": "± 233",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 4161,
            "range": "± 242",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7973,
            "range": "± 198",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 8578,
            "range": "± 3374",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 24888,
            "range": "± 822",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 87688,
            "range": "± 1379",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 435,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 556,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 3950,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24442,
            "range": "± 56",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101478,
            "range": "± 342",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 451,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 134874,
            "range": "± 8076",
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
          "id": "07ba1d8cafbf162c7be81e3ade3dcddb52f82d02",
          "message": "chore: bump versions for release -- iii(iii/v0.11.7-next.3)",
          "timestamp": "2026-05-15T17:01:06Z",
          "tree_id": "df66bdd91b3612ae6da0ffe05eca0db3d1242391",
          "url": "https://github.com/iii-hq/iii/commit/07ba1d8cafbf162c7be81e3ade3dcddb52f82d02"
        },
        "date": 1778866188399,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2677,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 17814,
            "range": "± 263",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 79080,
            "range": "± 356",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 307718,
            "range": "± 10354",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 47378,
            "range": "± 1015",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 141463,
            "range": "± 864",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 93032,
            "range": "± 1554",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 482754,
            "range": "± 1231",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3193248,
            "range": "± 92071",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 923844,
            "range": "± 5356",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2596337,
            "range": "± 25629",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21223402,
            "range": "± 838472",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4869532,
            "range": "± 68995",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 1945,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 556863,
            "range": "± 10121",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42803522,
            "range": "± 153238",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 48022609,
            "range": "± 375228",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 52198204,
            "range": "± 7975039",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 276755,
            "range": "± 15218",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 299125,
            "range": "± 9129",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 543564,
            "range": "± 14060",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 4118835,
            "range": "± 513843",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 272177,
            "range": "± 16665",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1447,
            "range": "± 72",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 1764,
            "range": "± 39",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8423,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 194529,
            "range": "± 563",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1147,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 428,
            "range": "± 1",
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
            "value": 1059,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 670,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 623,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1503,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 391,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1534,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6088,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32446,
            "range": "± 141",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 41013,
            "range": "± 132",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 492,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 387,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 32,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 31,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 221,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 223,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 387,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 413,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 256,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 74,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 81,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 82,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 644,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 668,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1272,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1335,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 516,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 176,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 97,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 98,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 929,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 956,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1784,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1878,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 818,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 245,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 32126,
            "range": "± 1191",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 50142,
            "range": "± 580",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 101746,
            "range": "± 1166",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 372282,
            "range": "± 7716",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 7736,
            "range": "± 151",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 7763,
            "range": "± 101",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27103,
            "range": "± 267",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 106576,
            "range": "± 1988",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 429994,
            "range": "± 1588",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6700,
            "range": "± 61",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7652,
            "range": "± 73",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 16547,
            "range": "± 88",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 293368,
            "range": "± 679",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1441,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 457,
            "range": "± 0",
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
            "value": 1616,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 416,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 40820,
            "range": "± 56",
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
            "value": 30812,
            "range": "± 1491",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 53845,
            "range": "± 1072",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 139013,
            "range": "± 1205",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 543901,
            "range": "± 11974",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 6855,
            "range": "± 1349",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 19445,
            "range": "± 1694",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 64945,
            "range": "± 4003",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 3119,
            "range": "± 270",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 4179,
            "range": "± 287",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7913,
            "range": "± 207",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 9321,
            "range": "± 4747",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28358,
            "range": "± 1170",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 95896,
            "range": "± 2990",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 429,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 571,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 3919,
            "range": "± 58",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24393,
            "range": "± 61",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101686,
            "range": "± 347",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 440,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 129738,
            "range": "± 8942",
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
          "id": "34da736bea363efc142c126070c98b980ae7f6dd",
          "message": "chore: bump versions for release -- iii(iii/v0.11.7-next.4)",
          "timestamp": "2026-05-15T18:06:50Z",
          "tree_id": "dcbaa4a80fbd71ba4ccd406ee0d696f0d2af5be7",
          "url": "https://github.com/iii-hq/iii/commit/34da736bea363efc142c126070c98b980ae7f6dd"
        },
        "date": 1778870009642,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2370,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 16418,
            "range": "± 101",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 70925,
            "range": "± 216",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 283301,
            "range": "± 629",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 37297,
            "range": "± 1373",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 113334,
            "range": "± 1107",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 72966,
            "range": "± 1473",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 368016,
            "range": "± 581",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3453583,
            "range": "± 22653",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 724073,
            "range": "± 4220",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 1898423,
            "range": "± 3872",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 19843520,
            "range": "± 351225",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 3677212,
            "range": "± 119638",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 1811,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 425048,
            "range": "± 4010",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42003183,
            "range": "± 168586",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45510818,
            "range": "± 267531",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 52487773,
            "range": "± 1919433",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 241408,
            "range": "± 4312",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 261230,
            "range": "± 2250",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 414248,
            "range": "± 3375",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2076468,
            "range": "± 48480",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 239141,
            "range": "± 1338",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1501,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 1998,
            "range": "± 29",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 7297,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 67438,
            "range": "± 498",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 935,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 279,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 82,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 780,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 535,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 489,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1098,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 347,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1358,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 5384,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 26849,
            "range": "± 48",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 31625,
            "range": "± 360",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 308,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 229,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 32,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 30,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 200,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 182,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 333,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 329,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 230,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 73,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 86,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 86,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 693,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 666,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1167,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1298,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 528,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 171,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 101,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 101,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 943,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 873,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1541,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1701,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 729,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 228,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 25279,
            "range": "± 505",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 40298,
            "range": "± 1025",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 75783,
            "range": "± 2419",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 253808,
            "range": "± 11871",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 6711,
            "range": "± 148",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 6802,
            "range": "± 113",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 23131,
            "range": "± 108",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 90689,
            "range": "± 212",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 369223,
            "range": "± 1370",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 5932,
            "range": "± 65",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 6618,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 13973,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 96850,
            "range": "± 214",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1142,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 305,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1177,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 376,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 30943,
            "range": "± 135",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 85,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 25105,
            "range": "± 360",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 42337,
            "range": "± 288",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 99721,
            "range": "± 1227",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 367622,
            "range": "± 4771",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4086,
            "range": "± 391",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 13482,
            "range": "± 370",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 47303,
            "range": "± 2105",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 1808,
            "range": "± 92",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 2648,
            "range": "± 106",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 5847,
            "range": "± 112",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 6196,
            "range": "± 172",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 22446,
            "range": "± 310",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 77986,
            "range": "± 2025",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 511,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 647,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4737,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 23936,
            "range": "± 45",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 98789,
            "range": "± 611",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 514,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 114989,
            "range": "± 7156",
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
          "id": "68479fd0e455a56041fa60678905d6af8b4c2288",
          "message": "chore: bump versions for release -- iii(iii/v0.12.0-next.1)",
          "timestamp": "2026-05-15T20:04:39Z",
          "tree_id": "46ce803b53d3301409596f34d15c96be02e146fd",
          "url": "https://github.com/iii-hq/iii/commit/68479fd0e455a56041fa60678905d6af8b4c2288"
        },
        "date": 1778876272900,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2769,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18785,
            "range": "± 408",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 82735,
            "range": "± 609",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 333184,
            "range": "± 3539",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44373,
            "range": "± 902",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 126880,
            "range": "± 671",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 85398,
            "range": "± 1004",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 464839,
            "range": "± 7177",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3854664,
            "range": "± 46103",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 859279,
            "range": "± 3944",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2383505,
            "range": "± 13634",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 23252252,
            "range": "± 638952",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4338062,
            "range": "± 20541",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2087,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 547362,
            "range": "± 12978",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42622588,
            "range": "± 219275",
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
          "id": "880d2d5ce06d81e4b39fa30bff561fd652cffe8d",
          "message": "chore: bump versions for release -- iii(iii/v0.14.0-next.1)",
          "timestamp": "2026-05-15T20:30:15Z",
          "tree_id": "53cfe9b8d52d77c4a0c2d26eb4ddca028a292325",
          "url": "https://github.com/iii-hq/iii/commit/880d2d5ce06d81e4b39fa30bff561fd652cffe8d"
        },
        "date": 1778878729434,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2805,
            "range": "± 124",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19275,
            "range": "± 452",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 84279,
            "range": "± 242",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 337874,
            "range": "± 1543",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45412,
            "range": "± 1669",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 127939,
            "range": "± 519",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 84119,
            "range": "± 1005",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 454603,
            "range": "± 3163",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3990863,
            "range": "± 22014",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 851990,
            "range": "± 1954",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2360458,
            "range": "± 18121",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21576070,
            "range": "± 613910",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4290528,
            "range": "± 15839",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2133,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 542513,
            "range": "± 19017",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42020046,
            "range": "± 74543",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46060227,
            "range": "± 4117546",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 55840980,
            "range": "± 1865955",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 333458,
            "range": "± 17677",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 358468,
            "range": "± 16282",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 548429,
            "range": "± 19698",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2381217,
            "range": "± 81154",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 338822,
            "range": "± 16741",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1712,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2307,
            "range": "± 39",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8648,
            "range": "± 74",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 104863,
            "range": "± 1319",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1118,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 356,
            "range": "± 10",
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
            "value": 994,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 663,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 595,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1376,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 432,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1598,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6133,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 31999,
            "range": "± 132",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38577,
            "range": "± 168",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 404,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 283,
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
            "value": 40,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 235,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 234,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 389,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 415,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 246,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 81,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 102,
            "range": "± 1",
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
            "value": 764,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 778,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1371,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1494,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 602,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 218,
            "range": "± 0",
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
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1006,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1036,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1830,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1967,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 860,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 266,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41626,
            "range": "± 1066",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59145,
            "range": "± 451",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 108530,
            "range": "± 2918",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 307477,
            "range": "± 7621",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8259,
            "range": "± 151",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8267,
            "range": "± 156",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27818,
            "range": "± 193",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108659,
            "range": "± 848",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 436290,
            "range": "± 6899",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7094,
            "range": "± 94",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7911,
            "range": "± 59",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 52436,
            "range": "± 249",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 152894,
            "range": "± 796",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1376,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 376,
            "range": "± 1",
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
            "value": 1479,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 433,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37845,
            "range": "± 206",
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
            "value": 42219,
            "range": "± 1937",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 64796,
            "range": "± 426",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 131458,
            "range": "± 1583",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 453618,
            "range": "± 8521",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4924,
            "range": "± 57921",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16073,
            "range": "± 32703",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56079,
            "range": "± 19513",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2109,
            "range": "± 56",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3115,
            "range": "± 80",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6936,
            "range": "± 160",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7246,
            "range": "± 61054",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26616,
            "range": "± 71222",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 93026,
            "range": "± 2682",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 548,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 688,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4845,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 25041,
            "range": "± 106",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101675,
            "range": "± 445",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 553,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 211609,
            "range": "± 20693",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ytallo123@hotmail.com",
            "name": "Ytallo",
            "username": "ytallo"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f9e4bf03080140666b0af4c4ffd3990f7ab44eda",
          "message": "chore: bump versions for release -- iii(iii/v0.12.0) (#1650)",
          "timestamp": "2026-05-16T13:43:14-03:00",
          "tree_id": "7bbe0ee7925ca92ae82bea5676d95dede5647440",
          "url": "https://github.com/iii-hq/iii/commit/f9e4bf03080140666b0af4c4ffd3990f7ab44eda"
        },
        "date": 1778951816844,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2738,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18769,
            "range": "± 348",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 82347,
            "range": "± 204",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 332119,
            "range": "± 1463",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 43831,
            "range": "± 982",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 126690,
            "range": "± 248",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86445,
            "range": "± 921",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 464180,
            "range": "± 1055",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3778290,
            "range": "± 24850",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 863101,
            "range": "± 12156",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2377364,
            "range": "± 13904",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21964595,
            "range": "± 554928",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4334474,
            "range": "± 26839",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2133,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 546880,
            "range": "± 6542",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42206297,
            "range": "± 240785",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46562269,
            "range": "± 1670008",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57011454,
            "range": "± 10090103",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 330582,
            "range": "± 18782",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 354943,
            "range": "± 19292",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 548939,
            "range": "± 18192",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2343142,
            "range": "± 55706",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 329990,
            "range": "± 19437",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1662,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2273,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 12844,
            "range": "± 174",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85992,
            "range": "± 2107",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1127,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 354,
            "range": "± 1",
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
            "value": 982,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 658,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 602,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1369,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 404,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1602,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6433,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32616,
            "range": "± 145",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38059,
            "range": "± 163",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 402,
            "range": "± 3",
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
            "value": 39,
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
            "value": 252,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 232,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 448,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 456,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 281,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 93,
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
            "value": 103,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 841,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 894,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1482,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1567,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 682,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 232,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 122,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 122,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1041,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1188,
            "range": "± 67",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2031,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2093,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 971,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 276,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 40231,
            "range": "± 1479",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58177,
            "range": "± 485",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 107396,
            "range": "± 4437",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 302180,
            "range": "± 8662",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8075,
            "range": "± 219",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8188,
            "range": "± 182",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27545,
            "range": "± 154",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 107691,
            "range": "± 380",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 432950,
            "range": "± 3333",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6998,
            "range": "± 91",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7901,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 52752,
            "range": "± 292",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123994,
            "range": "± 785",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1371,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 378,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 130,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1448,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 428,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37527,
            "range": "± 471",
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
            "value": 41728,
            "range": "± 1958",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 63815,
            "range": "± 393",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 128723,
            "range": "± 1205",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 443938,
            "range": "± 5955",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5007,
            "range": "± 654",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16612,
            "range": "± 462",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57756,
            "range": "± 2073",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2167,
            "range": "± 290",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3140,
            "range": "± 156",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6878,
            "range": "± 157",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7747,
            "range": "± 421",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 27346,
            "range": "± 799",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 94777,
            "range": "± 4095",
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
            "value": 700,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4839,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 25114,
            "range": "± 72",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 102262,
            "range": "± 664",
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
            "value": 203055,
            "range": "± 20616",
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
          "id": "5893e1a39b2ac45faaed35d3d503d7dea603b504",
          "message": "chore: bump versions for release -- iii(iii/v0.13.0-next.1)",
          "timestamp": "2026-05-21T17:49:51Z",
          "tree_id": "04124abb1a926ef0f1e841f7d2c6c0f1e0d888f0",
          "url": "https://github.com/iii-hq/iii/commit/5893e1a39b2ac45faaed35d3d503d7dea603b504"
        },
        "date": 1779387498291,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2592,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 16962,
            "range": "± 175",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 75369,
            "range": "± 995",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 296668,
            "range": "± 3641",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 48650,
            "range": "± 1442",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 147637,
            "range": "± 1241",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 92025,
            "range": "± 1791",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 482319,
            "range": "± 866",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3021725,
            "range": "± 75160",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 903281,
            "range": "± 1282",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2578144,
            "range": "± 39300",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 20739896,
            "range": "± 899094",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4753694,
            "range": "± 64649",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 1948,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 553746,
            "range": "± 21613",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42912575,
            "range": "± 128387",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47962352,
            "range": "± 766449",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58782742,
            "range": "± 3262132",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 266186,
            "range": "± 10359",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 298110,
            "range": "± 8285",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 550423,
            "range": "± 15595",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 3334056,
            "range": "± 185427",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 268808,
            "range": "± 8297",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1383,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 1759,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8736,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 208114,
            "range": "± 285",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1152,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 427,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 109,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1060,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 665,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 624,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1486,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 379,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1505,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 5974,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32577,
            "range": "± 57",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 40972,
            "range": "± 134",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 492,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 386,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 33,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 33,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 230,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 224,
            "range": "± 0",
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
            "value": 380,
            "range": "± 1",
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
            "value": 76,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 81,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 80,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 648,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 673,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1279,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1332,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 536,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 175,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 99,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 99,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 929,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 923,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1760,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1828,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 804,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 243,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 32167,
            "range": "± 4180",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 49028,
            "range": "± 1682",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 101889,
            "range": "± 5806",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 364520,
            "range": "± 7501",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 7786,
            "range": "± 130",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 7925,
            "range": "± 105",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27384,
            "range": "± 191",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108472,
            "range": "± 1361",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 435270,
            "range": "± 2474",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6879,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7913,
            "range": "± 83",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 16375,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 317565,
            "range": "± 682",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1455,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 460,
            "range": "± 0",
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
            "value": 1594,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 417,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 40445,
            "range": "± 246",
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
            "value": 31852,
            "range": "± 1697",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 53754,
            "range": "± 576",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 147971,
            "range": "± 2111",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 536009,
            "range": "± 11253",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 6602,
            "range": "± 694",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 19016,
            "range": "± 752",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 63912,
            "range": "± 2891",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 3104,
            "range": "± 443",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 4121,
            "range": "± 229",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7932,
            "range": "± 175",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 8583,
            "range": "± 3412",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28157,
            "range": "± 527",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 95862,
            "range": "± 3179",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 431,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 556,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 3978,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24323,
            "range": "± 178",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101641,
            "range": "± 368",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 441,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 133226,
            "range": "± 8555",
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
          "id": "3583e95fb0b5d398744d78f86b92f51ec9d03046",
          "message": "chore: bump versions for release -- iii(iii/v0.13.0)",
          "timestamp": "2026-05-25T14:14:15Z",
          "tree_id": "8afd5189d3aa41d16fd510e8fe5afe5fafa2bead",
          "url": "https://github.com/iii-hq/iii/commit/3583e95fb0b5d398744d78f86b92f51ec9d03046"
        },
        "date": 1779720173883,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3053,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20843,
            "range": "± 164",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 89588,
            "range": "± 668",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 360783,
            "range": "± 1876",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 48759,
            "range": "± 2255",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 143990,
            "range": "± 526",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88910,
            "range": "± 2360",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 479418,
            "range": "± 3489",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4447431,
            "range": "± 48317",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 871985,
            "range": "± 1987",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2497463,
            "range": "± 34537",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25086010,
            "range": "± 608366",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4440427,
            "range": "± 16154",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2302,
            "range": "± 61",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 529330,
            "range": "± 15206",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42318712,
            "range": "± 195807",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46790560,
            "range": "± 1225915",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57093385,
            "range": "± 2595348",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 311571,
            "range": "± 9151",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 335846,
            "range": "± 11005",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 530961,
            "range": "± 18759",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2465275,
            "range": "± 75368",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 314349,
            "range": "± 7940",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1889,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2558,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9359,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86359,
            "range": "± 456",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1261,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 491,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 106,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1061,
            "range": "± 46",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 662,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 626,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1388,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 423,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1689,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6622,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 33698,
            "range": "± 179",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 41878,
            "range": "± 2889",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 393,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 291,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 39,
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
            "value": 259,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 233,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 407,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 409,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 256,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 92,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 105,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 111,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 829,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 893,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1519,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1683,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 665,
            "range": "± 7",
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
            "value": 120,
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
            "value": 1106,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1127,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2045,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2281,
            "range": "± 50",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 924,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 282,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33292,
            "range": "± 649",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52758,
            "range": "± 434",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 97541,
            "range": "± 2425",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 325452,
            "range": "± 11044",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8459,
            "range": "± 227",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8621,
            "range": "± 135",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29265,
            "range": "± 242",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 115003,
            "range": "± 435",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 469070,
            "range": "± 1281",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7366,
            "range": "± 49",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8238,
            "range": "± 71",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17454,
            "range": "± 102",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123123,
            "range": "± 2173",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1437,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 393,
            "range": "± 0",
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
            "value": 1409,
            "range": "± 38",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 485,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 40296,
            "range": "± 214",
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
            "value": 33920,
            "range": "± 1604",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 55977,
            "range": "± 1022",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 124093,
            "range": "± 1785",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 454217,
            "range": "± 7571",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5307,
            "range": "± 470",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18006,
            "range": "± 368",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 63293,
            "range": "± 2393",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2256,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3349,
            "range": "± 141",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7478,
            "range": "± 180",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7972,
            "range": "± 587",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28481,
            "range": "± 802",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 97658,
            "range": "± 3881",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 663,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 812,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5833,
            "range": "± 93",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29926,
            "range": "± 228",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 122713,
            "range": "± 1212",
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
            "value": 149400,
            "range": "± 7878",
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
          "id": "a7ebbbbeaa9502efaf084b7692d6422a89b10169",
          "message": "chore: bump versions for release -- iii(iii/v0.14.0-next.2)",
          "timestamp": "2026-05-25T20:09:32Z",
          "tree_id": "68cc0f67a1b4f631b05dbedf5efc2abd265a5da1",
          "url": "https://github.com/iii-hq/iii/commit/a7ebbbbeaa9502efaf084b7692d6422a89b10169"
        },
        "date": 1779741573136,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2838,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19592,
            "range": "± 274",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 83976,
            "range": "± 727",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 331775,
            "range": "± 950",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44922,
            "range": "± 901",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 126982,
            "range": "± 2062",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86786,
            "range": "± 1635",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 474899,
            "range": "± 4020",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3859693,
            "range": "± 33885",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 875191,
            "range": "± 4764",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2442640,
            "range": "± 26377",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 22978002,
            "range": "± 495167",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4381602,
            "range": "± 62028",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2129,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 556095,
            "range": "± 10313",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42822159,
            "range": "± 415838",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46391323,
            "range": "± 325885",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57966061,
            "range": "± 3360524",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 343038,
            "range": "± 19040",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 361427,
            "range": "± 12914",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 567114,
            "range": "± 18254",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2385130,
            "range": "± 105067",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 344359,
            "range": "± 18644",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1692,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2350,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 10399,
            "range": "± 1162",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 104545,
            "range": "± 1359",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1160,
            "range": "± 2",
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
            "value": 101,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1044,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 662,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 586,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1387,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 414,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1632,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6454,
            "range": "± 139",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 31320,
            "range": "± 148",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38383,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 417,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 283,
            "range": "± 1",
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
            "value": 228,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 222,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 400,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 402,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 236,
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
            "value": 102,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 103,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 760,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 808,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1381,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1541,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 620,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 212,
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
            "value": 132,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 976,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1059,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1817,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2058,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 873,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 274,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41503,
            "range": "± 1170",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59457,
            "range": "± 882",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 108105,
            "range": "± 3226",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 305125,
            "range": "± 6042",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8111,
            "range": "± 132",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8169,
            "range": "± 129",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 28117,
            "range": "± 169",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108202,
            "range": "± 1305",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 440822,
            "range": "± 3196",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7088,
            "range": "± 141",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8022,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 51237,
            "range": "± 229",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 153910,
            "range": "± 9337",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1376,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 391,
            "range": "± 1",
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
            "value": 1338,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 443,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37526,
            "range": "± 90",
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
            "value": 42262,
            "range": "± 2201",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 63950,
            "range": "± 454",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 128891,
            "range": "± 2561",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 444843,
            "range": "± 5887",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5036,
            "range": "± 400",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16593,
            "range": "± 250",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 58189,
            "range": "± 2018",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2090,
            "range": "± 80",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3067,
            "range": "± 88",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6858,
            "range": "± 188",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7229,
            "range": "± 93",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26775,
            "range": "± 526",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 93378,
            "range": "± 2118",
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
            "value": 686,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4842,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24969,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101690,
            "range": "± 2921",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 554,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 200046,
            "range": "± 21498",
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
          "id": "b1b5ad7586bbe20ea66ebd401f379d0d050f8152",
          "message": "chore: bump versions for release -- iii(iii/v0.15.0-next.1)",
          "timestamp": "2026-05-26T14:49:39Z",
          "tree_id": "4120b8ed2554584179d2a51ed01bb03183c3104c",
          "url": "https://github.com/iii-hq/iii/commit/b1b5ad7586bbe20ea66ebd401f379d0d050f8152"
        },
        "date": 1779808776184,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3062,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20539,
            "range": "± 125",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 88887,
            "range": "± 648",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 362122,
            "range": "± 5640",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 48862,
            "range": "± 1912",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 144239,
            "range": "± 779",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88109,
            "range": "± 854",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 491468,
            "range": "± 6890",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4610621,
            "range": "± 51176",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 870042,
            "range": "± 8094",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2533398,
            "range": "± 19997",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25070832,
            "range": "± 715141",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4449865,
            "range": "± 55450",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2305,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 528978,
            "range": "± 7404",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42432928,
            "range": "± 210516",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46719687,
            "range": "± 101225",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56002035,
            "range": "± 3471102",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 315782,
            "range": "± 52533",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 339878,
            "range": "± 3345",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 535189,
            "range": "± 5567",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2523515,
            "range": "± 87377",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 314904,
            "range": "± 8463",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1891,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2556,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9331,
            "range": "± 40",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86145,
            "range": "± 1302",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1171,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 358,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 109,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 999,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 667,
            "range": "± 44",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 636,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1393,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 427,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1681,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6618,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 33651,
            "range": "± 70",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 42573,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 422,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 295,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 40,
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
            "value": 224,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 224,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 377,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 404,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 240,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 81,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 107,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 107,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 838,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 891,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1525,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1691,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 643,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 224,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 125,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 124,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1062,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1128,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1951,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2189,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 900,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 279,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33931,
            "range": "± 659",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 53023,
            "range": "± 306",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 100178,
            "range": "± 2694",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 323167,
            "range": "± 9945",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8547,
            "range": "± 140",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8533,
            "range": "± 151",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 29056,
            "range": "± 134",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 116441,
            "range": "± 652",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 470541,
            "range": "± 2250",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7351,
            "range": "± 67",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8416,
            "range": "± 72",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 18004,
            "range": "± 54",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123735,
            "range": "± 432",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1420,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 392,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 130,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1392,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 466,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38886,
            "range": "± 351",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 120,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 33772,
            "range": "± 525",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 57086,
            "range": "± 393",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 122587,
            "range": "± 2098",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 476405,
            "range": "± 8127",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5375,
            "range": "± 495",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 18159,
            "range": "± 462",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 63328,
            "range": "± 2761",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2241,
            "range": "± 109",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3325,
            "range": "± 123",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7475,
            "range": "± 229",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7859,
            "range": "± 65",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28796,
            "range": "± 462",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 97024,
            "range": "± 3257",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 667,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 803,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5922,
            "range": "± 107",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29691,
            "range": "± 70",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 122280,
            "range": "± 455",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 662,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 153669,
            "range": "± 9070",
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
          "id": "25c26e7f6c1b7eea83cf277d4a0ef7e251e05bf3",
          "message": "chore: bump versions for release -- iii(iii/v0.16.0-next.1)",
          "timestamp": "2026-05-26T17:46:05Z",
          "tree_id": "b6a6b8df86eed26c600eeaf7fc680483f6eb3b83",
          "url": "https://github.com/iii-hq/iii/commit/25c26e7f6c1b7eea83cf277d4a0ef7e251e05bf3"
        },
        "date": 1779819388294,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2813,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19443,
            "range": "± 617",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 84077,
            "range": "± 1530",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 334886,
            "range": "± 3115",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 46205,
            "range": "± 1729",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 132471,
            "range": "± 1041",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86808,
            "range": "± 1436",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 465880,
            "range": "± 9060",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4050209,
            "range": "± 129989",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 866650,
            "range": "± 3975",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2399625,
            "range": "± 32435",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 24350288,
            "range": "± 1147191",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4382031,
            "range": "± 63654",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2145,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 581163,
            "range": "± 14698",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42896074,
            "range": "± 200603",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47539657,
            "range": "± 743988",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57306034,
            "range": "± 3603276",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 349716,
            "range": "± 18755",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 360373,
            "range": "± 10224",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 558654,
            "range": "± 17649",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2504263,
            "range": "± 253278",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 339496,
            "range": "± 16444",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1698,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2378,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8657,
            "range": "± 45",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 97670,
            "range": "± 7286",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1134,
            "range": "± 58",
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
            "value": 103,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1016,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 658,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 606,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1331,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 395,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1579,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6218,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 33354,
            "range": "± 187",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38281,
            "range": "± 129",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 377,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 286,
            "range": "± 12",
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
            "value": 246,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 225,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 398,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 403,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 241,
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
            "value": 101,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 101,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 761,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 808,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1403,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1503,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 629,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 212,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 129,
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
            "value": 1022,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1075,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1861,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1986,
            "range": "± 70",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 868,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 284,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41791,
            "range": "± 4032",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59750,
            "range": "± 668",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 108473,
            "range": "± 6672",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 307150,
            "range": "± 7811",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8236,
            "range": "± 176",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8167,
            "range": "± 115",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27713,
            "range": "± 126",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108333,
            "range": "± 1317",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 440787,
            "range": "± 10440",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7068,
            "range": "± 62",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8170,
            "range": "± 69",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 52577,
            "range": "± 779",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123242,
            "range": "± 7735",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1404,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 422,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 128,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1445,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 429,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37469,
            "range": "± 112",
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
            "value": 42153,
            "range": "± 3109",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 64167,
            "range": "± 928",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 130974,
            "range": "± 1770",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 442281,
            "range": "± 4827",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5201,
            "range": "± 1863",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16999,
            "range": "± 2762",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 59636,
            "range": "± 9878",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2305,
            "range": "± 559",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3283,
            "range": "± 645",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7042,
            "range": "± 518",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7596,
            "range": "± 1117",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26891,
            "range": "± 1551",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 94021,
            "range": "± 4379",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 545,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 684,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4833,
            "range": "± 60",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24981,
            "range": "± 144",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 102714,
            "range": "± 1443",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 548,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 199103,
            "range": "± 25278",
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
          "id": "67f31961c59c010823d861333f11bb598e43740d",
          "message": "chore: bump versions for release -- iii(iii/v0.16.0-next.2)",
          "timestamp": "2026-05-26T20:22:52Z",
          "tree_id": "60859238865f40b3d2e4246030bba9729b23c58a",
          "url": "https://github.com/iii-hq/iii/commit/67f31961c59c010823d861333f11bb598e43740d"
        },
        "date": 1779828779918,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2829,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18861,
            "range": "± 425",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 84025,
            "range": "± 425",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 334751,
            "range": "± 1493",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44003,
            "range": "± 1051",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133425,
            "range": "± 553",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 92033,
            "range": "± 739",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 457746,
            "range": "± 5758",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3751932,
            "range": "± 27852",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 912233,
            "range": "± 5609",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2358110,
            "range": "± 31566",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21872140,
            "range": "± 554333",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4587258,
            "range": "± 25192",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2110,
            "range": "± 149",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 558407,
            "range": "± 9889",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42595015,
            "range": "± 154887",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46808143,
            "range": "± 742841",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56920008,
            "range": "± 3220481",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 352163,
            "range": "± 18693",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 363559,
            "range": "± 9768",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 560602,
            "range": "± 15441",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2467898,
            "range": "± 59233",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 342726,
            "range": "± 17187",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1672,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2273,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 10761,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 105556,
            "range": "± 762",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1132,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 372,
            "range": "± 2",
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
            "value": 993,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 661,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 602,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1366,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 402,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1590,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6184,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32567,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 39196,
            "range": "± 109",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 429,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 282,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 38,
            "range": "± 1",
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
            "value": 254,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 270,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 466,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 484,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 295,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 98,
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
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 762,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 776,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1350,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1519,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 594,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 213,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 122,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 123,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1039,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1078,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1898,
            "range": "± 67",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2035,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 913,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 287,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 40684,
            "range": "± 1190",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58299,
            "range": "± 381",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 106193,
            "range": "± 3809",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 306932,
            "range": "± 6257",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8167,
            "range": "± 160",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8189,
            "range": "± 94",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27533,
            "range": "± 305",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 106355,
            "range": "± 458",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 434256,
            "range": "± 6408",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6931,
            "range": "± 157",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7987,
            "range": "± 39",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50936,
            "range": "± 647",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123084,
            "range": "± 1095",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1366,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 386,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 129,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1433,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 442,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37989,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 104,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 41986,
            "range": "± 2360",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 63388,
            "range": "± 293",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 128565,
            "range": "± 1554",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 442492,
            "range": "± 7169",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4977,
            "range": "± 459",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16295,
            "range": "± 513",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57299,
            "range": "± 2009",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2186,
            "range": "± 81",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3282,
            "range": "± 107",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7264,
            "range": "± 183",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7180,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26562,
            "range": "± 472",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 92412,
            "range": "± 3373",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 542,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 690,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4852,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24900,
            "range": "± 103",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 103140,
            "range": "± 814",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 562,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 207903,
            "range": "± 21034",
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
          "id": "576f738bace11dcdc1b4d48eff9bb732fa9d645a",
          "message": "chore: bump versions for release -- iii(iii/v0.16.0-next.3)",
          "timestamp": "2026-05-27T12:32:49Z",
          "tree_id": "1d92af912865f067ec720771a6fbc2445bb89c9b",
          "url": "https://github.com/iii-hq/iii/commit/576f738bace11dcdc1b4d48eff9bb732fa9d645a"
        },
        "date": 1779886841399,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2383,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 16339,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 70771,
            "range": "± 576",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 286184,
            "range": "± 3459",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 36792,
            "range": "± 1308",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 114191,
            "range": "± 889",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 68540,
            "range": "± 207",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 370304,
            "range": "± 2638",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3604932,
            "range": "± 152091",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 678092,
            "range": "± 10911",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 1902373,
            "range": "± 29616",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 19840889,
            "range": "± 513844",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 3450008,
            "range": "± 46918",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 1824,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 416711,
            "range": "± 12202",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42029260,
            "range": "± 48898",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 45394462,
            "range": "± 322252",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 52374790,
            "range": "± 9726757",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 242344,
            "range": "± 1807",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 262072,
            "range": "± 1495",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 413522,
            "range": "± 3958",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 1935567,
            "range": "± 36359",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 242885,
            "range": "± 9647",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1487,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2013,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 7284,
            "range": "± 104",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 67582,
            "range": "± 152",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 929,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 285,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 84,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 787,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 539,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 486,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1084,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 342,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1368,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 5295,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 27847,
            "range": "± 56",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 32118,
            "range": "± 88",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 332,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 220,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 32,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 31,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 190,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 184,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 344,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 361,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 228,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 72,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 86,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 86,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 689,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 661,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1171,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1269,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 491,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 170,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 102,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 914,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 889,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1593,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 1702,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 725,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 228,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 26558,
            "range": "± 588",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 41620,
            "range": "± 240",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 78703,
            "range": "± 2281",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 256024,
            "range": "± 9912",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 6618,
            "range": "± 104",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 6639,
            "range": "± 203",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 23119,
            "range": "± 305",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 90652,
            "range": "± 1270",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 365176,
            "range": "± 9289",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 5661,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 6571,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 15618,
            "range": "± 686",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 97026,
            "range": "± 1096",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1147,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 303,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 106,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1108,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 380,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 30555,
            "range": "± 73",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 87,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 26724,
            "range": "± 1014",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 44653,
            "range": "± 616",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 101673,
            "range": "± 1052",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 369084,
            "range": "± 8824",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4271,
            "range": "± 1005",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 13901,
            "range": "± 1037",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 47984,
            "range": "± 3106",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 1880,
            "range": "± 226",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 2690,
            "range": "± 300",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 5956,
            "range": "± 295",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 6325,
            "range": "± 385",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 22571,
            "range": "± 599",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 78023,
            "range": "± 2756",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 515,
            "range": "± 44",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 625,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4539,
            "range": "± 69",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 23218,
            "range": "± 764",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 96346,
            "range": "± 585",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 519,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 117282,
            "range": "± 5774",
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
          "id": "2011aaba19f597cdaa6c06b3d0cd711023263cec",
          "message": "chore: bump versions for release -- iii(iii/v0.16.0-next.4)",
          "timestamp": "2026-05-27T19:59:14Z",
          "tree_id": "4150a08fa2aed1136e2399e5d160088786dc30df",
          "url": "https://github.com/iii-hq/iii/commit/2011aaba19f597cdaa6c06b3d0cd711023263cec"
        },
        "date": 1779912849956,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 3067,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20359,
            "range": "± 570",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 91054,
            "range": "± 546",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 367266,
            "range": "± 5603",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 46971,
            "range": "± 961",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 145908,
            "range": "± 6235",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88333,
            "range": "± 807",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 493378,
            "range": "± 1282",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 5005671,
            "range": "± 75431",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 877527,
            "range": "± 14125",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2663129,
            "range": "± 74917",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 29821647,
            "range": "± 502825",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4807413,
            "range": "± 126629",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2324,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 544102,
            "range": "± 13708",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 43003306,
            "range": "± 534405",
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
          "id": "2e705a40c30ed655a8b0be62b998ce29076d8ae1",
          "message": "chore: bump versions for release -- iii(iii/v0.16.0-next.5)",
          "timestamp": "2026-05-28T19:44:38Z",
          "tree_id": "95c8e9a0fb31decc2ccdf61d5ecc1f50e1ae14ca",
          "url": "https://github.com/iii-hq/iii/commit/2e705a40c30ed655a8b0be62b998ce29076d8ae1"
        },
        "date": 1779999309576,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2781,
            "range": "± 35",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19345,
            "range": "± 363",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 84110,
            "range": "± 212",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 335780,
            "range": "± 4654",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44693,
            "range": "± 1485",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 126214,
            "range": "± 858",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 84900,
            "range": "± 1606",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 465899,
            "range": "± 1474",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3842539,
            "range": "± 186884",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 854593,
            "range": "± 3390",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2397688,
            "range": "± 39615",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21939971,
            "range": "± 645707",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4305024,
            "range": "± 15102",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2072,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 552898,
            "range": "± 10600",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42792510,
            "range": "± 253817",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47187890,
            "range": "± 318669",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 55411558,
            "range": "± 3078169",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 338078,
            "range": "± 16192",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 360868,
            "range": "± 11598",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 566999,
            "range": "± 16917",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2446965,
            "range": "± 100835",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 337878,
            "range": "± 15224",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1647,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2320,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8794,
            "range": "± 29",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 106623,
            "range": "± 1753",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1151,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 349,
            "range": "± 1",
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
            "value": 996,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 661,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 641,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1359,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 406,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1628,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6166,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32493,
            "range": "± 1990",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38683,
            "range": "± 92",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 414,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 285,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 39,
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
            "value": 257,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 264,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 448,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 476,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 280,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 92,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 100,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 100,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 778,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 783,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1360,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1490,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 607,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 210,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 129,
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
            "value": 1049,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1076,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1876,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2086,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 916,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 278,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41214,
            "range": "± 1531",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58697,
            "range": "± 1322",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 106878,
            "range": "± 2419",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 309485,
            "range": "± 7613",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8096,
            "range": "± 142",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8085,
            "range": "± 236",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27436,
            "range": "± 193",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 107027,
            "range": "± 495",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 432798,
            "range": "± 3424",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6956,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7932,
            "range": "± 141",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50715,
            "range": "± 361",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123523,
            "range": "± 4348",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1336,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 382,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 126,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1404,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 428,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37465,
            "range": "± 155",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 111,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 42174,
            "range": "± 1471",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 65561,
            "range": "± 1391",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 130595,
            "range": "± 2358",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 444004,
            "range": "± 6123",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4924,
            "range": "± 428",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16182,
            "range": "± 201",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56782,
            "range": "± 3338",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2170,
            "range": "± 93",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3105,
            "range": "± 89",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6925,
            "range": "± 141",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7317,
            "range": "± 120",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26587,
            "range": "± 295",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 93083,
            "range": "± 2706",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 562,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 676,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4885,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24323,
            "range": "± 713",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101033,
            "range": "± 3069",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 560,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 195165,
            "range": "± 17536",
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
          "id": "b8cfbc7d61a787af0b38551dd4bdb7ed05384cc5",
          "message": "chore: bump versions for release -- iii(iii/v0.16.0)",
          "timestamp": "2026-05-28T20:29:56Z",
          "tree_id": "ec639486e084d8d4f2f7818ba2f2aae99f592209",
          "url": "https://github.com/iii-hq/iii/commit/b8cfbc7d61a787af0b38551dd4bdb7ed05384cc5"
        },
        "date": 1780002013098,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2772,
            "range": "± 144",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19067,
            "range": "± 582",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 83477,
            "range": "± 232",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 331724,
            "range": "± 956",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45133,
            "range": "± 1363",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133064,
            "range": "± 472",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87311,
            "range": "± 1640",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 461522,
            "range": "± 2880",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3775010,
            "range": "± 32574",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 866481,
            "range": "± 4565",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2389283,
            "range": "± 22736",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 22178211,
            "range": "± 703950",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4374540,
            "range": "± 19187",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2109,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 562864,
            "range": "± 10203",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42379361,
            "range": "± 124152",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46744547,
            "range": "± 380758",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58141632,
            "range": "± 3692409",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 331803,
            "range": "± 18077",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 354556,
            "range": "± 14185",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 548197,
            "range": "± 16537",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2390985,
            "range": "± 117581",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 336429,
            "range": "± 18699",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1624,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2263,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8658,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85983,
            "range": "± 455",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1140,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 354,
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
            "value": 1016,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 640,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 591,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1362,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 402,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1614,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6283,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32962,
            "range": "± 112",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38565,
            "range": "± 101",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 425,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 285,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 37,
            "range": "± 1",
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
            "value": 226,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 230,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 394,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 415,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 249,
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
            "value": 756,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 811,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1410,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1508,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 617,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 215,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 129,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 128,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1039,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1064,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1813,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2043,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 894,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 289,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 39694,
            "range": "± 1202",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 60791,
            "range": "± 565",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 105871,
            "range": "± 2645",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 304578,
            "range": "± 10592",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8307,
            "range": "± 140",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8272,
            "range": "± 88",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27808,
            "range": "± 437",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108567,
            "range": "± 462",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 434322,
            "range": "± 1979",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7109,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8139,
            "range": "± 147",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 51793,
            "range": "± 250",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123947,
            "range": "± 498",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1359,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 376,
            "range": "± 2",
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
            "value": 1462,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 429,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 36661,
            "range": "± 86",
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
            "value": 39607,
            "range": "± 3296",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 63942,
            "range": "± 1803",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 129124,
            "range": "± 1614",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 448755,
            "range": "± 10383",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4935,
            "range": "± 569",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16145,
            "range": "± 590",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56601,
            "range": "± 2320",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2159,
            "range": "± 136",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3112,
            "range": "± 90",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6893,
            "range": "± 123",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7330,
            "range": "± 246",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26511,
            "range": "± 363",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 92660,
            "range": "± 2729",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 561,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 676,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4861,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24327,
            "range": "± 56",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 100122,
            "range": "± 548",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 561,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 196688,
            "range": "± 20692",
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
          "id": "b67c98add77bad320ea23138224a7d674ad5f21e",
          "message": "chore: bump versions for release -- iii(iii/v0.16.1)",
          "timestamp": "2026-05-29T13:14:49Z",
          "tree_id": "796bfb6227f810239001585a36290f35dd50b3d2",
          "url": "https://github.com/iii-hq/iii/commit/b67c98add77bad320ea23138224a7d674ad5f21e"
        },
        "date": 1780061350966,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2990,
            "range": "± 61",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20716,
            "range": "± 273",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 92582,
            "range": "± 1049",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 364484,
            "range": "± 4951",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 48057,
            "range": "± 1117",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 144722,
            "range": "± 917",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 89474,
            "range": "± 1537",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 492873,
            "range": "± 3344",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4526946,
            "range": "± 94448",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 873220,
            "range": "± 6277",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2556148,
            "range": "± 27615",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 27120042,
            "range": "± 1118591",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4450241,
            "range": "± 35009",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2335,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 536582,
            "range": "± 5895",
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
          "id": "ce44ac0f7eae69c8956c3d4e8296663ba93589fb",
          "message": "chore: bump versions for release -- iii(iii/v0.16.2-next.1)",
          "timestamp": "2026-05-29T19:27:57Z",
          "tree_id": "7567ec9072338b7f1c3cd58a7a5db60e92b18471",
          "url": "https://github.com/iii-hq/iii/commit/ce44ac0f7eae69c8956c3d4e8296663ba93589fb"
        },
        "date": 1780084693118,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2821,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19382,
            "range": "± 600",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 85007,
            "range": "± 1387",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 337223,
            "range": "± 1297",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 47069,
            "range": "± 1569",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133897,
            "range": "± 707",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87644,
            "range": "± 2317",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 463637,
            "range": "± 3154",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3845805,
            "range": "± 23347",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 868333,
            "range": "± 4549",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2371501,
            "range": "± 20832",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21728355,
            "range": "± 521466",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4363805,
            "range": "± 19410",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2209,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 549834,
            "range": "± 10934",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42118107,
            "range": "± 142962",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46303905,
            "range": "± 323806",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56668481,
            "range": "± 3766090",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 336323,
            "range": "± 18568",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 358194,
            "range": "± 10261",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 550197,
            "range": "± 20177",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2399816,
            "range": "± 96228",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 351413,
            "range": "± 18027",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1662,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2276,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9152,
            "range": "± 29",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 104935,
            "range": "± 788",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1135,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 350,
            "range": "± 7",
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
            "value": 1005,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 657,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 592,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1359,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 407,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1605,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6343,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 30996,
            "range": "± 211",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38739,
            "range": "± 106",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 397,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 280,
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
            "value": 202,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 220,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 377,
            "range": "± 3",
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
            "value": 243,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 79,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 101,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 101,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 759,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 835,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1388,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1487,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 620,
            "range": "± 1",
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
            "value": 130,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1009,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1049,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1795,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2005,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 904,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 271,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41070,
            "range": "± 1900",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58998,
            "range": "± 421",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 107389,
            "range": "± 2817",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 310674,
            "range": "± 6830",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8098,
            "range": "± 161",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8199,
            "range": "± 169",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27664,
            "range": "± 205",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 109479,
            "range": "± 869",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 442583,
            "range": "± 2277",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6873,
            "range": "± 32",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8094,
            "range": "± 81",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 52108,
            "range": "± 440",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 122626,
            "range": "± 495",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1338,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 370,
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
            "value": 1446,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 431,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37530,
            "range": "± 104",
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
            "value": 41862,
            "range": "± 2220",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 67049,
            "range": "± 1113",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 131609,
            "range": "± 1640",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 451370,
            "range": "± 5786",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5019,
            "range": "± 684",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16473,
            "range": "± 782",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 60235,
            "range": "± 5802",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2270,
            "range": "± 383",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3140,
            "range": "± 153",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6928,
            "range": "± 193",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7306,
            "range": "± 178",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26661,
            "range": "± 460",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 92652,
            "range": "± 2536",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 564,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 674,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4884,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24492,
            "range": "± 109",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 101113,
            "range": "± 1386",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 562,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 196109,
            "range": "± 16844",
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
          "id": "c4d38ab893d9e19b8ca1c4440f99225bfd3b2059",
          "message": "chore: bump versions for release -- iii(iii/v0.17.0-next.1)",
          "timestamp": "2026-06-01T16:02:49Z",
          "tree_id": "1598d62b7d71bc69f91c67670a34ccdb66e448f4",
          "url": "https://github.com/iii-hq/iii/commit/c4d38ab893d9e19b8ca1c4440f99225bfd3b2059"
        },
        "date": 1780331597705,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2800,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19063,
            "range": "± 348",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 83338,
            "range": "± 601",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 331207,
            "range": "± 4112",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 46519,
            "range": "± 1948",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 131808,
            "range": "± 1026",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86610,
            "range": "± 4718",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 460660,
            "range": "± 5580",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3836292,
            "range": "± 30456",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 856294,
            "range": "± 5402",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2377469,
            "range": "± 23451",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 23694058,
            "range": "± 1323247",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4310714,
            "range": "± 29137",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2152,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 561547,
            "range": "± 19946",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42687423,
            "range": "± 332550",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46625660,
            "range": "± 461894",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58307606,
            "range": "± 1598820",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 347565,
            "range": "± 17562",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 364449,
            "range": "± 7635",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 567200,
            "range": "± 20286",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2501226,
            "range": "± 74076",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 337340,
            "range": "± 16120",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1672,
            "range": "± 69",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2306,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9152,
            "range": "± 46",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 104712,
            "range": "± 501",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1131,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 362,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 102,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 974,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 651,
            "range": "± 91",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 604,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1347,
            "range": "± 62",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 420,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1671,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 6675,
            "range": "± 77",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 32039,
            "range": "± 214",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 38609,
            "range": "± 206",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 402,
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
            "value": 38,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 238,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 245,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 423,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 440,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 263,
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
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 101,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 793,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 789,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1346,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1578,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 601,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 214,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 132,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 128,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1041,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1033,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 1912,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2037,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 899,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 276,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41916,
            "range": "± 1499",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59801,
            "range": "± 552",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 108124,
            "range": "± 3652",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 309662,
            "range": "± 9974",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8154,
            "range": "± 164",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8058,
            "range": "± 138",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27690,
            "range": "± 171",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108344,
            "range": "± 1252",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 438537,
            "range": "± 5736",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6923,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7920,
            "range": "± 323",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 53940,
            "range": "± 1597",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 151609,
            "range": "± 810",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1370,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 403,
            "range": "± 2",
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
            "value": 1371,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 434,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37241,
            "range": "± 123",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 105,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 42889,
            "range": "± 1933",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 66321,
            "range": "± 810",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 128677,
            "range": "± 1734",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 446885,
            "range": "± 8601",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4959,
            "range": "± 422",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16303,
            "range": "± 355",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57089,
            "range": "± 2096",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2141,
            "range": "± 59",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3074,
            "range": "± 69",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6761,
            "range": "± 103",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7151,
            "range": "± 109",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26420,
            "range": "± 349",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 92080,
            "range": "± 2826",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 551,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 669,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4839,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24534,
            "range": "± 63",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 100805,
            "range": "± 671",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 554,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 194533,
            "range": "± 19645",
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
          "id": "ad5ff667709e301313f650b687db5cec5887b808",
          "message": "chore: bump versions for release -- iii(iii/v0.17.0)",
          "timestamp": "2026-06-01T19:02:48Z",
          "tree_id": "4e5d27f7455047bb157f70755cb23df0a92bd88b",
          "url": "https://github.com/iii-hq/iii/commit/ad5ff667709e301313f650b687db5cec5887b808"
        },
        "date": 1780342878576,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2621,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18365,
            "range": "± 295",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 77924,
            "range": "± 473",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 320914,
            "range": "± 661",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 43487,
            "range": "± 1533",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 128181,
            "range": "± 586",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87938,
            "range": "± 2209",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 435260,
            "range": "± 2001",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3745127,
            "range": "± 43395",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 859202,
            "range": "± 3774",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2256284,
            "range": "± 20835",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21691037,
            "range": "± 871201",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4356276,
            "range": "± 26647",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2089,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 513359,
            "range": "± 9792",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42605445,
            "range": "± 306082",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46688987,
            "range": "± 484465",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56499029,
            "range": "± 4368078",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 297869,
            "range": "± 16492",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 323018,
            "range": "± 14493",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 508495,
            "range": "± 15104",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2325480,
            "range": "± 111763",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 299749,
            "range": "± 15810",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1622,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2198,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8739,
            "range": "± 56",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86212,
            "range": "± 381",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1134,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 363,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 113,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1052,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 721,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 628,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1464,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 475,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1863,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7384,
            "range": "± 50",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 33972,
            "range": "± 156",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37547,
            "range": "± 87",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 438,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 301,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 64,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 67,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 428,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 436,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 690,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 724,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 432,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 135,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 101,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 101,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 808,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 824,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1507,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1553,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 686,
            "range": "± 2",
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
            "value": 157,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 153,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1254,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1253,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2349,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2414,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1153,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 340,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 36633,
            "range": "± 1721",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 55896,
            "range": "± 628",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 99019,
            "range": "± 2485",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 298207,
            "range": "± 6642",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8208,
            "range": "± 145",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8115,
            "range": "± 105",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27387,
            "range": "± 158",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 107296,
            "range": "± 624",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 425865,
            "range": "± 1804",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7061,
            "range": "± 57",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7825,
            "range": "± 71",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 51855,
            "range": "± 705",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 154624,
            "range": "± 1687",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1374,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 389,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 135,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1428,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 500,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37025,
            "range": "± 219",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 121,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 36612,
            "range": "± 1760",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 59689,
            "range": "± 1863",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 119210,
            "range": "± 3195",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 419138,
            "range": "± 6827",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4929,
            "range": "± 770",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16371,
            "range": "± 791",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57442,
            "range": "± 2450",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2225,
            "range": "± 323",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3213,
            "range": "± 275",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6585,
            "range": "± 174",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 9640,
            "range": "± 2033",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26086,
            "range": "± 715",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 90581,
            "range": "± 2951",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 586,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 676,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4897,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24508,
            "range": "± 77",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 100425,
            "range": "± 621",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 591,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 188228,
            "range": "± 16364",
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
          "id": "7a78d89ecfa8136ce243e876a5521d059be2f68c",
          "message": "chore: bump versions for release -- iii(iii/v0.18.0-next.1)",
          "timestamp": "2026-06-01T20:41:43Z",
          "tree_id": "87d3045936c1333912e335b2898b455faa2e2b56",
          "url": "https://github.com/iii-hq/iii/commit/7a78d89ecfa8136ce243e876a5521d059be2f68c"
        },
        "date": 1780348838299,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2945,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20484,
            "range": "± 194",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 85918,
            "range": "± 1365",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 344522,
            "range": "± 5182",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45381,
            "range": "± 1807",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 139074,
            "range": "± 1573",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86845,
            "range": "± 1561",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 444488,
            "range": "± 6691",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4362896,
            "range": "± 79520",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 890412,
            "range": "± 10215",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2311320,
            "range": "± 35693",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25726951,
            "range": "± 813174",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4541962,
            "range": "± 22032",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2331,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 493596,
            "range": "± 12105",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42531841,
            "range": "± 268660",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46899801,
            "range": "± 827375",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57530344,
            "range": "± 3285813",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 288382,
            "range": "± 5518",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 313412,
            "range": "± 2543",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 497443,
            "range": "± 3882",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2497690,
            "range": "± 80787",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 289738,
            "range": "± 6018",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1819,
            "range": "± 35",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2447,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9229,
            "range": "± 298",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85894,
            "range": "± 254",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1165,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 401,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 123,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1076,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 724,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 649,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1421,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 456,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1849,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7107,
            "range": "± 91",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34562,
            "range": "± 96",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 40640,
            "range": "± 628",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 422,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 316,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 74,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 74,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 473,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 488,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 762,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 820,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 488,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 168,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 120,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 120,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 876,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 837,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1628,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1653,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 740,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 226,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 147,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 146,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1314,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1336,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2356,
            "range": "± 106",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2570,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1183,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 357,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33685,
            "range": "± 643",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 53029,
            "range": "± 690",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 98339,
            "range": "± 6302",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 316333,
            "range": "± 14899",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8325,
            "range": "± 151",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8504,
            "range": "± 157",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 28445,
            "range": "± 139",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 109149,
            "range": "± 551",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 441088,
            "range": "± 8211",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7073,
            "range": "± 56",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7857,
            "range": "± 145",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17109,
            "range": "± 142",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123041,
            "range": "± 324",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1445,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 400,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 139,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1422,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 488,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38234,
            "range": "± 4084",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 122,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 33262,
            "range": "± 1031",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 56300,
            "range": "± 1756",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 124479,
            "range": "± 6175",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 449764,
            "range": "± 8077",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5518,
            "range": "± 2587",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 17992,
            "range": "± 1736",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 64977,
            "range": "± 14449",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2421,
            "range": "± 461",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3340,
            "range": "± 1180",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6923,
            "range": "± 340",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 8395,
            "range": "± 1786",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28137,
            "range": "± 1470",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 94087,
            "range": "± 4294",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 677,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 787,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5842,
            "range": "± 127",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 28906,
            "range": "± 62",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 119070,
            "range": "± 1948",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 681,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 140563,
            "range": "± 7239",
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
          "id": "bb2f74c2ae4bd5bb27f864bdc7a9a2c4efbcbcf7",
          "message": "chore: bump versions for release -- iii(iii/v0.18.0)",
          "timestamp": "2026-06-03T17:03:38Z",
          "tree_id": "96a1b947892329862b2441dcfda6aa377648c926",
          "url": "https://github.com/iii-hq/iii/commit/bb2f74c2ae4bd5bb27f864bdc7a9a2c4efbcbcf7"
        },
        "date": 1780508525079,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2661,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18416,
            "range": "± 357",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 79550,
            "range": "± 175",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 326480,
            "range": "± 1522",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44838,
            "range": "± 1483",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 127926,
            "range": "± 519",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88602,
            "range": "± 3492",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 454442,
            "range": "± 6007",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3706033,
            "range": "± 14959",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 872376,
            "range": "± 4513",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2333397,
            "range": "± 12899",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 20473804,
            "range": "± 466909",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4360423,
            "range": "± 151856",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2088,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 525299,
            "range": "± 14269",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42200172,
            "range": "± 191571",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46974706,
            "range": "± 560056",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56098107,
            "range": "± 4171462",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 332625,
            "range": "± 19647",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 348958,
            "range": "± 18375",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 538268,
            "range": "± 17487",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2620173,
            "range": "± 180263",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 324852,
            "range": "± 11734",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1620,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2310,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8542,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 104723,
            "range": "± 382",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1119,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 383,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 113,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1042,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 716,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 657,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1468,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 449,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1785,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7216,
            "range": "± 40",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34025,
            "range": "± 217",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37510,
            "range": "± 1013",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 425,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 297,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 64,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 61,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 394,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 420,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 647,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 712,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 413,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 133,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 108,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 108,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 828,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 838,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1449,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1609,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 708,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 238,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 139,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 137,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1209,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1287,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2229,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2395,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1146,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 331,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 40991,
            "range": "± 1155",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 57309,
            "range": "± 403",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 103984,
            "range": "± 3338",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 292872,
            "range": "± 9782",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8109,
            "range": "± 135",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 7996,
            "range": "± 135",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27364,
            "range": "± 192",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 106717,
            "range": "± 553",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 430789,
            "range": "± 2992",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6963,
            "range": "± 111",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7840,
            "range": "± 51",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50534,
            "range": "± 235",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 159393,
            "range": "± 11525",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1462,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 407,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 136,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1492,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 516,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37643,
            "range": "± 93",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 119,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 41773,
            "range": "± 2157",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 60723,
            "range": "± 659",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 123347,
            "range": "± 2180",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 440697,
            "range": "± 13157",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4971,
            "range": "± 414",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16347,
            "range": "± 701",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57090,
            "range": "± 1955",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2174,
            "range": "± 86",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3024,
            "range": "± 80",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6500,
            "range": "± 95",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7092,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26007,
            "range": "± 360",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 90588,
            "range": "± 2854",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 568,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 664,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4861,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24311,
            "range": "± 78",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 99753,
            "range": "± 437",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 565,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 181960,
            "range": "± 21804",
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
          "id": "21587e0d2d66eeda8c8b2ee8cdcc35ce7788a7f5",
          "message": "chore: bump versions for release -- iii(iii/v0.19.0-next.1)",
          "timestamp": "2026-06-04T17:14:27Z",
          "tree_id": "813132a3b670b9fbf989c85aa79b98755dac8c87",
          "url": "https://github.com/iii-hq/iii/commit/21587e0d2d66eeda8c8b2ee8cdcc35ce7788a7f5"
        },
        "date": 1780595586660,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2878,
            "range": "± 74",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 20025,
            "range": "± 528",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 85212,
            "range": "± 226",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 348559,
            "range": "± 6114",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44191,
            "range": "± 1260",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133936,
            "range": "± 434",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87370,
            "range": "± 2541",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 450451,
            "range": "± 1059",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4348256,
            "range": "± 81846",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 865793,
            "range": "± 3222",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2332306,
            "range": "± 9934",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 24996808,
            "range": "± 1071446",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4402076,
            "range": "± 19277",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2324,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 495427,
            "range": "± 2867",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42374377,
            "range": "± 114997",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47104006,
            "range": "± 693365",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58137622,
            "range": "± 5617840",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 290118,
            "range": "± 5426",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 316244,
            "range": "± 2086",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 500963,
            "range": "± 4882",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2545253,
            "range": "± 66323",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 288628,
            "range": "± 6104",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1844,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2559,
            "range": "± 60",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9350,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86250,
            "range": "± 203",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1186,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 404,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 119,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1091,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 722,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 655,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1431,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 459,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1805,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7233,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 35037,
            "range": "± 344",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 40760,
            "range": "± 317",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 451,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 312,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 71,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 69,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 425,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 439,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 687,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 753,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 425,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 140,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 119,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 119,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 866,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 853,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1554,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1590,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 715,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 229,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 141,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 140,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1262,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1291,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2316,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2523,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1142,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 334,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33581,
            "range": "± 649",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 53347,
            "range": "± 641",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 99601,
            "range": "± 7774",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 316020,
            "range": "± 18622",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8392,
            "range": "± 195",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8493,
            "range": "± 142",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 28136,
            "range": "± 167",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 109967,
            "range": "± 529",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 446525,
            "range": "± 1466",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7067,
            "range": "± 60",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8018,
            "range": "± 59",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17244,
            "range": "± 153",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123610,
            "range": "± 3147",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1420,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 405,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 138,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1454,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 498,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38865,
            "range": "± 445",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 120,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 32822,
            "range": "± 428",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 55464,
            "range": "± 386",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 125046,
            "range": "± 2351",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 462230,
            "range": "± 12731",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5258,
            "range": "± 729",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 17556,
            "range": "± 815",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 61068,
            "range": "± 3143",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2295,
            "range": "± 128",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3318,
            "range": "± 164",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 7040,
            "range": "± 139",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7740,
            "range": "± 149",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 27714,
            "range": "± 413",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 94331,
            "range": "± 3183",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 667,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 792,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5849,
            "range": "± 116",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 28474,
            "range": "± 678",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 117500,
            "range": "± 673",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 670,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 139831,
            "range": "± 5510",
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
          "id": "83a08c24b75ba428c161ab1823aee790717d6d13",
          "message": "chore: bump versions for release -- iii(iii/v0.19.0-next.2)",
          "timestamp": "2026-06-04T19:04:08Z",
          "tree_id": "ceea25fab0d3996267c4b575962267e1dad537e1",
          "url": "https://github.com/iii-hq/iii/commit/83a08c24b75ba428c161ab1823aee790717d6d13"
        },
        "date": 1780602127686,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2841,
            "range": "± 41",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18844,
            "range": "± 248",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 80476,
            "range": "± 372",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 331590,
            "range": "± 4693",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44067,
            "range": "± 1567",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 128205,
            "range": "± 354",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88121,
            "range": "± 1842",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 442008,
            "range": "± 3386",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3760311,
            "range": "± 27869",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 866774,
            "range": "± 4188",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2291704,
            "range": "± 17279",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 22149884,
            "range": "± 700671",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4401661,
            "range": "± 15921",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2217,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 537912,
            "range": "± 10793",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42953428,
            "range": "± 117399",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47038098,
            "range": "± 465744",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57198727,
            "range": "± 2047137",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 326928,
            "range": "± 17215",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 359488,
            "range": "± 18573",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 534560,
            "range": "± 23190",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2416918,
            "range": "± 89903",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 320016,
            "range": "± 15518",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1724,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2290,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8544,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86032,
            "range": "± 366",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1117,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 371,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 114,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1044,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 700,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 635,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1392,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 459,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1799,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7264,
            "range": "± 86",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34718,
            "range": "± 427",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37620,
            "range": "± 95",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 417,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 305,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 66,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 66,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 439,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 480,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 750,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 788,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 468,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 150,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 102,
            "range": "± 1",
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
            "value": 825,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 833,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1508,
            "range": "± 85",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1656,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 741,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 220,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 157,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 155,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1283,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1297,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2378,
            "range": "± 77",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2556,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1242,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 342,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41015,
            "range": "± 1314",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58643,
            "range": "± 508",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 103614,
            "range": "± 2310",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 293270,
            "range": "± 10617",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8296,
            "range": "± 139",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8237,
            "range": "± 127",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27808,
            "range": "± 306",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108617,
            "range": "± 297",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 434732,
            "range": "± 1728",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7163,
            "range": "± 62",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8142,
            "range": "± 92",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 52798,
            "range": "± 218",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123876,
            "range": "± 470",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1453,
            "range": "± 53",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 402,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 135,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1433,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 496,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37707,
            "range": "± 112",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 117,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 41469,
            "range": "± 1910",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 61129,
            "range": "± 679",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 124479,
            "range": "± 1753",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 426410,
            "range": "± 12596",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4942,
            "range": "± 26683",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16285,
            "range": "± 19555",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57152,
            "range": "± 15365",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2119,
            "range": "± 67",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 2998,
            "range": "± 90",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6443,
            "range": "± 92",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7107,
            "range": "± 102",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 25899,
            "range": "± 270",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 88996,
            "range": "± 2154",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 560,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 662,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4897,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24043,
            "range": "± 169",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 99603,
            "range": "± 823",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 560,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 182860,
            "range": "± 20226",
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
          "id": "3eaa3e28db3e1119669c8adf464fa02fb635f6fc",
          "message": "chore: bump versions for release -- iii(iii/v0.19.0-next.3)",
          "timestamp": "2026-06-05T17:16:56Z",
          "tree_id": "f22649226e6f663c1716a04bb98a20c06d15ead6",
          "url": "https://github.com/iii-hq/iii/commit/3eaa3e28db3e1119669c8adf464fa02fb635f6fc"
        },
        "date": 1780682135958,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2651,
            "range": "± 51",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18371,
            "range": "± 94",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 79564,
            "range": "± 375",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 320988,
            "range": "± 1530",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 42351,
            "range": "± 1000",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 123218,
            "range": "± 625",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 85711,
            "range": "± 895",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 444270,
            "range": "± 2364",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3989418,
            "range": "± 29247",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 869200,
            "range": "± 2732",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2284380,
            "range": "± 10541",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 23491340,
            "range": "± 520954",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4403183,
            "range": "± 19188",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2065,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 538223,
            "range": "± 10253",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42650978,
            "range": "± 292225",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47228374,
            "range": "± 390837",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57430438,
            "range": "± 4881388",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 330170,
            "range": "± 19712",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 350789,
            "range": "± 15389",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 551266,
            "range": "± 17802",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2535479,
            "range": "± 161434",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 328495,
            "range": "± 11919",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1580,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2199,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8817,
            "range": "± 38",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86799,
            "range": "± 352",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1125,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 374,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 121,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1061,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 699,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 640,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1426,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 447,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1796,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7126,
            "range": "± 49",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 33905,
            "range": "± 356",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37788,
            "range": "± 102",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 413,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 297,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 66,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 66,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 442,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 454,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 739,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 798,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 459,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 150,
            "range": "± 1",
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
            "value": 109,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 872,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 843,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1511,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1649,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 735,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 223,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 145,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 143,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1345,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1326,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2328,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2540,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1199,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 349,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41843,
            "range": "± 1317",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58974,
            "range": "± 686",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 104601,
            "range": "± 2410",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 294613,
            "range": "± 5361",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8362,
            "range": "± 136",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8490,
            "range": "± 107",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 28378,
            "range": "± 253",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108754,
            "range": "± 531",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 439052,
            "range": "± 2574",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7198,
            "range": "± 85",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8287,
            "range": "± 121",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 51845,
            "range": "± 213",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123357,
            "range": "± 611",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1399,
            "range": "± 42",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 406,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 135,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1469,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 486,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 36726,
            "range": "± 137",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 121,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 41736,
            "range": "± 2182",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 61683,
            "range": "± 526",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 124402,
            "range": "± 1639",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 441032,
            "range": "± 6913",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5137,
            "range": "± 602",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16517,
            "range": "± 349",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56888,
            "range": "± 2438",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2321,
            "range": "± 141",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3184,
            "range": "± 98",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6747,
            "range": "± 121",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7527,
            "range": "± 352",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26362,
            "range": "± 351",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 91777,
            "range": "± 2854",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 559,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 677,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4850,
            "range": "± 50",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24338,
            "range": "± 55",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 99433,
            "range": "± 728",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 574,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 182744,
            "range": "± 19225",
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
          "id": "e90cfc5f17341a01e834375fb8b3060b0f90d057",
          "message": "chore: bump versions for release -- iii(iii/v0.19.0-next.4)",
          "timestamp": "2026-06-05T20:03:08Z",
          "tree_id": "51ee9d6032a76b3bea105dfb6038831f0441eab0",
          "url": "https://github.com/iii-hq/iii/commit/e90cfc5f17341a01e834375fb8b3060b0f90d057"
        },
        "date": 1780692107816,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2586,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 17922,
            "range": "± 580",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 76164,
            "range": "± 837",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 313937,
            "range": "± 3665",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44124,
            "range": "± 2160",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 127899,
            "range": "± 622",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 85865,
            "range": "± 1376",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 440737,
            "range": "± 3794",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3872225,
            "range": "± 35198",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 866088,
            "range": "± 13297",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2278734,
            "range": "± 36510",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 22738081,
            "range": "± 662017",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4340523,
            "range": "± 27235",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2046,
            "range": "± 116",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 526760,
            "range": "± 11581",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42764755,
            "range": "± 209929",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47146473,
            "range": "± 431819",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57099469,
            "range": "± 3969813",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 332346,
            "range": "± 18420",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 348472,
            "range": "± 13939",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 548120,
            "range": "± 17399",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2403162,
            "range": "± 105288",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 324018,
            "range": "± 14202",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1585,
            "range": "± 51",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2206,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8693,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 111330,
            "range": "± 491",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1173,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 376,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 122,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1077,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 702,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 645,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1458,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 463,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1837,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7721,
            "range": "± 32",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34833,
            "range": "± 209",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37672,
            "range": "± 117",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 449,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 300,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 66,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 63,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 474,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 481,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 801,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 830,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 490,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 149,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 116,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 119,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 835,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 870,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1498,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1587,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 698,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 228,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 148,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 142,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1343,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1358,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2386,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2565,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1202,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 340,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 41789,
            "range": "± 2649",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 58395,
            "range": "± 591",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 102585,
            "range": "± 3329",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 293794,
            "range": "± 5194",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8186,
            "range": "± 179",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8109,
            "range": "± 138",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27896,
            "range": "± 130",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 107953,
            "range": "± 1860",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 432243,
            "range": "± 15174",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7059,
            "range": "± 125",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8094,
            "range": "± 407",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50252,
            "range": "± 465",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 154072,
            "range": "± 4496",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1416,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 400,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 135,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1488,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 484,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37042,
            "range": "± 741",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 115,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 41626,
            "range": "± 1559",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 61351,
            "range": "± 834",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 124575,
            "range": "± 1594",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 432258,
            "range": "± 7292",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4937,
            "range": "± 509",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16206,
            "range": "± 1678",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56550,
            "range": "± 2451",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2167,
            "range": "± 184",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3060,
            "range": "± 180",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6567,
            "range": "± 90",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7150,
            "range": "± 80",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 25886,
            "range": "± 288",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 90601,
            "range": "± 2865",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 560,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 683,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4867,
            "range": "± 134",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 23965,
            "range": "± 55",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 100115,
            "range": "± 2205",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 562,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 176927,
            "range": "± 21822",
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
          "id": "d15fc9775558a99f7122b6d7a06dddba10c5a550",
          "message": "chore: bump versions for release -- iii(iii/v0.19.0)",
          "timestamp": "2026-06-06T00:09:27Z",
          "tree_id": "e84cc2a0e9fbdb9fd2d76415079c4fd856d570b3",
          "url": "https://github.com/iii-hq/iii/commit/d15fc9775558a99f7122b6d7a06dddba10c5a550"
        },
        "date": 1780706863358,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2558,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18162,
            "range": "± 507",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 77531,
            "range": "± 619",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 314103,
            "range": "± 1746",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44541,
            "range": "± 1600",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 127800,
            "range": "± 560",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87053,
            "range": "± 891",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 441488,
            "range": "± 10388",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3825112,
            "range": "± 72805",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 861208,
            "range": "± 2536",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2278981,
            "range": "± 15185",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21425399,
            "range": "± 424774",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4354605,
            "range": "± 17806",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2064,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 513005,
            "range": "± 24653",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42117522,
            "range": "± 108721",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46539434,
            "range": "± 493108",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56958786,
            "range": "± 6176341",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 320190,
            "range": "± 14212",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 343880,
            "range": "± 22170",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 522463,
            "range": "± 16329",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2207338,
            "range": "± 127546",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 317534,
            "range": "± 14552",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1587,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2160,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8943,
            "range": "± 41",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 104730,
            "range": "± 1013",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1134,
            "range": "± 12",
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
            "value": 119,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1050,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 698,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 656,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1437,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 440,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1778,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7083,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34177,
            "range": "± 144",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37424,
            "range": "± 307",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 409,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 299,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 65,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 64,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 428,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 435,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 707,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 751,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 455,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 134,
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
            "value": 896,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 860,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1556,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1690,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 744,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 225,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 159,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 155,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1295,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1295,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2358,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2503,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1221,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 337,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 39803,
            "range": "± 2317",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 57206,
            "range": "± 598",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 102189,
            "range": "± 3566",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 294697,
            "range": "± 4715",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8209,
            "range": "± 131",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8275,
            "range": "± 118",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27522,
            "range": "± 137",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 106673,
            "range": "± 566",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 426226,
            "range": "± 3529",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7168,
            "range": "± 92",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8005,
            "range": "± 122",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50839,
            "range": "± 311",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 155021,
            "range": "± 2457",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1411,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 392,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 136,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1454,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 492,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37529,
            "range": "± 89",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 115,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 40672,
            "range": "± 2271",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 60446,
            "range": "± 740",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 123109,
            "range": "± 1779",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 430144,
            "range": "± 7469",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4923,
            "range": "± 417",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16153,
            "range": "± 406",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56532,
            "range": "± 2283",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2196,
            "range": "± 314",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3061,
            "range": "± 97",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6581,
            "range": "± 160",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7516,
            "range": "± 512",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26167,
            "range": "± 1052",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 91201,
            "range": "± 3952",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 559,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 674,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4870,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24312,
            "range": "± 82",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 100690,
            "range": "± 445",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 564,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 174309,
            "range": "± 19801",
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
          "id": "35b8b2e23975f655d5c11bcfcc74485e96b17f01",
          "message": "chore: bump versions for release -- iii(iii/v0.19.1-next.1)",
          "timestamp": "2026-06-08T20:17:09Z",
          "tree_id": "c6bcaa8fa1091c8f165b40897d199baa26414f64",
          "url": "https://github.com/iii-hq/iii/commit/35b8b2e23975f655d5c11bcfcc74485e96b17f01"
        },
        "date": 1780952215383,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2628,
            "range": "± 60",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 17908,
            "range": "± 440",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 76431,
            "range": "± 2326",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 314561,
            "range": "± 6676",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 42955,
            "range": "± 1436",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 130178,
            "range": "± 2113",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 89021,
            "range": "± 1710",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 439243,
            "range": "± 3518",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4428728,
            "range": "± 213070",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 879213,
            "range": "± 9199",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2332718,
            "range": "± 41800",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 26134933,
            "range": "± 1460222",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4577491,
            "range": "± 146652",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2034,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 545923,
            "range": "± 15629",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42829604,
            "range": "± 166681",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46955492,
            "range": "± 578894",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57246132,
            "range": "± 2410018",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 333337,
            "range": "± 15534",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 352670,
            "range": "± 11964",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 557776,
            "range": "± 22919",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 3123363,
            "range": "± 625457",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 330516,
            "range": "± 14217",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1550,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2123,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8535,
            "range": "± 128",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 94002,
            "range": "± 7250",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1101,
            "range": "± 59",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 369,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 114,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1028,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 687,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 632,
            "range": "± 102",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1399,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 454,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1791,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7067,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34300,
            "range": "± 399",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37627,
            "range": "± 117",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 413,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 297,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 67,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 66,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 444,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 463,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 705,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 783,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 451,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 149,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 105,
            "range": "± 0",
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
            "value": 839,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 836,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1550,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1597,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 722,
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
            "value": 145,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 141,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1287,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1312,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2341,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2489,
            "range": "± 97",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1192,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 352,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 42786,
            "range": "± 3657",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59818,
            "range": "± 1122",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 102994,
            "range": "± 1662",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 301717,
            "range": "± 4802",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8187,
            "range": "± 269",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8167,
            "range": "± 218",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27427,
            "range": "± 776",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 107304,
            "range": "± 2658",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 430148,
            "range": "± 10999",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7065,
            "range": "± 66",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7902,
            "range": "± 122",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 51237,
            "range": "± 299",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 152107,
            "range": "± 11339",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1400,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 407,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 133,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1478,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 491,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37903,
            "range": "± 768",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 115,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 42610,
            "range": "± 2362",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 60400,
            "range": "± 675",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 122993,
            "range": "± 2884",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 436857,
            "range": "± 4988",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4989,
            "range": "± 22062",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16327,
            "range": "± 374",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 57430,
            "range": "± 2038",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2175,
            "range": "± 169",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3059,
            "range": "± 171",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6599,
            "range": "± 200",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7171,
            "range": "± 52100",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 25999,
            "range": "± 20556",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 90531,
            "range": "± 1951",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 558,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 679,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4900,
            "range": "± 71",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24347,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 100696,
            "range": "± 2089",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 571,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 182837,
            "range": "± 22961",
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
          "id": "9a46ae4f3225e4f478a471cbaa617c43908bbd76",
          "message": "chore: bump versions for release -- iii(iii/v0.19.2-next.1)",
          "timestamp": "2026-06-09T21:21:22Z",
          "tree_id": "bf0a5ee96e1c5bd8bf76a732411671b4ffcc5730",
          "url": "https://github.com/iii-hq/iii/commit/9a46ae4f3225e4f478a471cbaa617c43908bbd76"
        },
        "date": 1781042367593,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2544,
            "range": "± 87",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 17649,
            "range": "± 111",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 76260,
            "range": "± 377",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 310620,
            "range": "± 1798",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 43655,
            "range": "± 277",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 129123,
            "range": "± 688",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87163,
            "range": "± 1507",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 431638,
            "range": "± 2478",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3793440,
            "range": "± 22942",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 871072,
            "range": "± 2399",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2237154,
            "range": "± 38810",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21916162,
            "range": "± 479332",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4373408,
            "range": "± 9366",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2030,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 514184,
            "range": "± 16262",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42129132,
            "range": "± 174778",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46299466,
            "range": "± 385889",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 54816325,
            "range": "± 3355536",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 327190,
            "range": "± 16820",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 350295,
            "range": "± 19854",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 516729,
            "range": "± 21905",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2408819,
            "range": "± 66592",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 325838,
            "range": "± 17213",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1565,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2217,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8576,
            "range": "± 95",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85793,
            "range": "± 479",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1115,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 374,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 115,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1048,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 692,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 631,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1422,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 441,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1802,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7005,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 33784,
            "range": "± 122",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37889,
            "range": "± 91",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 470,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 300,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 66,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 63,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 442,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 459,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 785,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 804,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 468,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 149,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 111,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 110,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 802,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 824,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1518,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1605,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 721,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 226,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 150,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 146,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1317,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1296,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2389,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2542,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1254,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 349,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 40460,
            "range": "± 1670",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 57510,
            "range": "± 558",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 101492,
            "range": "± 1554",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 294544,
            "range": "± 7668",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8090,
            "range": "± 138",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8162,
            "range": "± 125",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27295,
            "range": "± 147",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 105826,
            "range": "± 417",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 429756,
            "range": "± 2687",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 6992,
            "range": "± 74",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 7977,
            "range": "± 80",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 51373,
            "range": "± 172",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 122725,
            "range": "± 624",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1413,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 397,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 137,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1473,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 520,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38118,
            "range": "± 140",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 115,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 41290,
            "range": "± 1761",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 59388,
            "range": "± 761",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 123986,
            "range": "± 1739",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 420719,
            "range": "± 5497",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4946,
            "range": "± 409",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16080,
            "range": "± 256",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56312,
            "range": "± 2236",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2128,
            "range": "± 49",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3065,
            "range": "± 88",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6620,
            "range": "± 89",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7097,
            "range": "± 88",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 25687,
            "range": "± 240",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 89847,
            "range": "± 2304",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 569,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 679,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4837,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24488,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 100609,
            "range": "± 544",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 562,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 177278,
            "range": "± 20492",
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
          "id": "92e39d08d88ae489e51b563f9107456b614b56ca",
          "message": "chore: bump versions for release -- iii(iii/v0.19.2)",
          "timestamp": "2026-06-09T22:17:30Z",
          "tree_id": "e60287f86e9d7587772a5adddde77029ef956160",
          "url": "https://github.com/iii-hq/iii/commit/92e39d08d88ae489e51b563f9107456b614b56ca"
        },
        "date": 1781045765184,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2609,
            "range": "± 42",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 17637,
            "range": "± 212",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 75680,
            "range": "± 523",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 315560,
            "range": "± 819",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44469,
            "range": "± 1594",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 122897,
            "range": "± 2354",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87592,
            "range": "± 1139",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 441068,
            "range": "± 2844",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3918046,
            "range": "± 25576",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 868455,
            "range": "± 12130",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2280911,
            "range": "± 30392",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 23871821,
            "range": "± 953882",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4376811,
            "range": "± 216822",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2067,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 508077,
            "range": "± 11298",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42451771,
            "range": "± 186039",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46418141,
            "range": "± 425435",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 53694938,
            "range": "± 3763269",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 298597,
            "range": "± 19254",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 320878,
            "range": "± 15942",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 501915,
            "range": "± 16195",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2347975,
            "range": "± 118793",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 299353,
            "range": "± 14951",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1553,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2172,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9315,
            "range": "± 53",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 104678,
            "range": "± 1111",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1145,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 387,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 115,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1034,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 712,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 651,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1420,
            "range": "± 171",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 447,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1755,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7142,
            "range": "± 60",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 34981,
            "range": "± 108",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37752,
            "range": "± 828",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 457,
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
            "value": 64,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 62,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 419,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 435,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 675,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 751,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 426,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 134,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 99,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 100,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 884,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 841,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1522,
            "range": "± 44",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1592,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 718,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 226,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 142,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 143,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1255,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1265,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2244,
            "range": "± 72",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2469,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1157,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 343,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 37297,
            "range": "± 1285",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 55758,
            "range": "± 709",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 97424,
            "range": "± 1655",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 288148,
            "range": "± 5559",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8227,
            "range": "± 135",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8063,
            "range": "± 115",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27687,
            "range": "± 162",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 106008,
            "range": "± 399",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 429825,
            "range": "± 7603",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7286,
            "range": "± 343",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8174,
            "range": "± 75",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 52074,
            "range": "± 281",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 122802,
            "range": "± 657",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1396,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 427,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 133,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1430,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 503,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37378,
            "range": "± 127",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 119,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 36757,
            "range": "± 1667",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 58002,
            "range": "± 1043",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 119546,
            "range": "± 1386",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 424635,
            "range": "± 5842",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 4973,
            "range": "± 1206",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16521,
            "range": "± 2377",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56536,
            "range": "± 2601",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2337,
            "range": "± 715",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3096,
            "range": "± 149",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6690,
            "range": "± 144",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7457,
            "range": "± 335",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 25965,
            "range": "± 378",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 90214,
            "range": "± 2999",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 562,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 698,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5100,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 25255,
            "range": "± 95",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 103923,
            "range": "± 674",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 566,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 168448,
            "range": "± 15487",
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
          "id": "7284e5b20ab902d395fd4e26b2fb99ae5fb1692a",
          "message": "chore: bump versions for release -- iii(iii/v0.19.3-next.1)",
          "timestamp": "2026-06-11T20:31:11Z",
          "tree_id": "4516a9ac171d5158c017962054d9cdf30b77bea4",
          "url": "https://github.com/iii-hq/iii/commit/7284e5b20ab902d395fd4e26b2fb99ae5fb1692a"
        },
        "date": 1781212243207,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2576,
            "range": "± 96",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 17713,
            "range": "± 135",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 76255,
            "range": "± 454",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 316374,
            "range": "± 2123",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 41735,
            "range": "± 1116",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 123241,
            "range": "± 1230",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88453,
            "range": "± 1823",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 434711,
            "range": "± 4105",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4588106,
            "range": "± 201685",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 886901,
            "range": "± 5589",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2307354,
            "range": "± 41822",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25984875,
            "range": "± 902871",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4614375,
            "range": "± 84122",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2046,
            "range": "± 65",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 551724,
            "range": "± 8974",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 43051424,
            "range": "± 108286",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47731922,
            "range": "± 360786",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58644870,
            "range": "± 3079770",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 326224,
            "range": "± 15397",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 346929,
            "range": "± 12206",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 551767,
            "range": "± 21057",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2520064,
            "range": "± 131151",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 323312,
            "range": "± 16426",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1558,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2197,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 8458,
            "range": "± 45",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 86167,
            "range": "± 502",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1118,
            "range": "± 4",
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
            "value": 113,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1054,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 695,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 642,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1430,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 457,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1841,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7169,
            "range": "± 61",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 35151,
            "range": "± 356",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 37738,
            "range": "± 243",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 456,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 297,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 67,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 63,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 443,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 443,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 734,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 784,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 442,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 139,
            "range": "± 1",
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
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 829,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 817,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1479,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1569,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 729,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 219,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 137,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 137,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1254,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1293,
            "range": "± 39",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2300,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2448,
            "range": "± 46",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1184,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 330,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 43484,
            "range": "± 1410",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 59421,
            "range": "± 676",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 104258,
            "range": "± 2808",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 290739,
            "range": "± 12770",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8207,
            "range": "± 163",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8126,
            "range": "± 152",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27686,
            "range": "± 733",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 106920,
            "range": "± 921",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 429284,
            "range": "± 2256",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7054,
            "range": "± 53",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8085,
            "range": "± 260",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 50971,
            "range": "± 293",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 153074,
            "range": "± 1555",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1428,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 385,
            "range": "± 2",
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
            "value": 1431,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 483,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 37693,
            "range": "± 923",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 115,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 42626,
            "range": "± 1750",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 61361,
            "range": "± 1221",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 123687,
            "range": "± 2212",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 439074,
            "range": "± 6837",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5000,
            "range": "± 515",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 16295,
            "range": "± 867",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 56983,
            "range": "± 1825",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2188,
            "range": "± 101",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3159,
            "range": "± 182",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6877,
            "range": "± 348",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7194,
            "range": "± 39447",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 26224,
            "range": "± 544",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 90608,
            "range": "± 3809",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 568,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 665,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 4905,
            "range": "± 39",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 24819,
            "range": "± 249",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 103127,
            "range": "± 1193",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 571,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 200623,
            "range": "± 17959",
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
          "id": "c20959648a66716eb5f199fe87f15224e277adfa",
          "message": "chore: bump versions for release -- iii(iii/v0.19.3)",
          "timestamp": "2026-06-15T13:59:52Z",
          "tree_id": "a003676e3bbba82efd5e9e26e17c211e05baa7b5",
          "url": "https://github.com/iii-hq/iii/commit/c20959648a66716eb5f199fe87f15224e277adfa"
        },
        "date": 1781534312372,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2776,
            "range": "± 56",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19715,
            "range": "± 62",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 83896,
            "range": "± 406",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 337291,
            "range": "± 1265",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 44155,
            "range": "± 1140",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 133896,
            "range": "± 468",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87645,
            "range": "± 2101",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 450562,
            "range": "± 6951",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4316404,
            "range": "± 37650",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 864708,
            "range": "± 8003",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2328305,
            "range": "± 22477",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 26171846,
            "range": "± 831786",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4421275,
            "range": "± 113292",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2279,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 503992,
            "range": "± 3440",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42360994,
            "range": "± 275771",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46952980,
            "range": "± 456302",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57152233,
            "range": "± 4733991",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1",
            "value": 292179,
            "range": "± 14794",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/10",
            "value": 318231,
            "range": "± 2358",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/100",
            "value": 502305,
            "range": "± 6389",
            "unit": "ns/iter"
          },
          {
            "name": "http_many_routes_loopback/1000",
            "value": 2503713,
            "range": "± 103924",
            "unit": "ns/iter"
          },
          {
            "name": "http_single_route_loopback/post_json",
            "value": 289378,
            "range": "± 4185",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1kb",
            "value": 1732,
            "range": "± 108",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/10kb",
            "value": 2400,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/100kb",
            "value": 9192,
            "range": "± 63",
            "unit": "ns/iter"
          },
          {
            "name": "invoke_function_payload_sizes/1mb",
            "value": 85796,
            "range": "± 235",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/set_overwrite",
            "value": 1147,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_hit",
            "value": 400,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/get_miss",
            "value": 120,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/delete",
            "value": 1099,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_set_field",
            "value": 748,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_increment",
            "value": 664,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/update_merge",
            "value": 1438,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/1",
            "value": 449,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/4",
            "value": 1790,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/16",
            "value": 7071,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store_contention/64",
            "value": 33723,
            "range": "± 109",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_100_items",
            "value": 40306,
            "range": "± 306",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_keys_with_prefix",
            "value": 466,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "kv_store/list_groups",
            "value": 320,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Ping",
            "value": 70,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/Pong",
            "value": 66,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterFunction",
            "value": 452,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/RegisterTrigger",
            "value": 471,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvokeFunction",
            "value": 745,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult",
            "value": 801,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/InvocationResult_Error",
            "value": 467,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_serialize/WorkerRegistered",
            "value": 158,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Ping",
            "value": 117,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/Pong",
            "value": 118,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterFunction",
            "value": 860,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/RegisterTrigger",
            "value": 849,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvokeFunction",
            "value": 1575,
            "range": "± 33",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult",
            "value": 1631,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/InvocationResult_Error",
            "value": 716,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_deserialize/WorkerRegistered",
            "value": 226,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Ping",
            "value": 140,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/Pong",
            "value": 141,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterFunction",
            "value": 1262,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/RegisterTrigger",
            "value": 1271,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvokeFunction",
            "value": 2305,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult",
            "value": 2446,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/InvocationResult_Error",
            "value": 1173,
            "range": "± 32",
            "unit": "ns/iter"
          },
          {
            "name": "protocol_roundtrip/WorkerRegistered",
            "value": 340,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/1",
            "value": 33790,
            "range": "± 688",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/8",
            "value": 52444,
            "range": "± 556",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/32",
            "value": 97292,
            "range": "± 3789",
            "unit": "ns/iter"
          },
          {
            "name": "pubsub_fanout/128",
            "value": 308280,
            "range": "± 5776",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue/push_single",
            "value": 8339,
            "range": "± 169",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/1",
            "value": 8405,
            "range": "± 133",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/4",
            "value": 27927,
            "range": "± 217",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/16",
            "value": 108576,
            "range": "± 522",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_concurrent/64",
            "value": 441481,
            "range": "± 3375",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1kb",
            "value": 7077,
            "range": "± 57",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/10kb",
            "value": 8045,
            "range": "± 94",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/100kb",
            "value": 17996,
            "range": "± 90",
            "unit": "ns/iter"
          },
          {
            "name": "queue_enqueue_payload_sizes/1mb",
            "value": 123309,
            "range": "± 1694",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/set_overwrite",
            "value": 1434,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_hit",
            "value": 399,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/get_miss",
            "value": 140,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/delete",
            "value": 1507,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/update_increment",
            "value": 479,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_100",
            "value": 38906,
            "range": "± 139",
            "unit": "ns/iter"
          },
          {
            "name": "state_adapter/list_groups",
            "value": 119,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/1",
            "value": 33446,
            "range": "± 719",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/8",
            "value": 56508,
            "range": "± 312",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/32",
            "value": 127299,
            "range": "± 3083",
            "unit": "ns/iter"
          },
          {
            "name": "trigger_fanout/128",
            "value": 461025,
            "range": "± 7683",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/10",
            "value": 5292,
            "range": "± 732",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/50",
            "value": 17828,
            "range": "± 673",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/functions/200",
            "value": 61552,
            "range": "± 4610",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/10",
            "value": 2255,
            "range": "± 145",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/50",
            "value": 3271,
            "range": "± 207",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/invocations/200",
            "value": 6855,
            "range": "± 198",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/10",
            "value": 7794,
            "range": "± 287",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/50",
            "value": 28208,
            "range": "± 486",
            "unit": "ns/iter"
          },
          {
            "name": "worker_cleanup/triggers/200",
            "value": 95369,
            "range": "± 3787",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_remove_sequential",
            "value": 663,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/1",
            "value": 792,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/8",
            "value": 5847,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/32",
            "value": 29308,
            "range": "± 110",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking_concurrent/128",
            "value": 121225,
            "range": "± 3935",
            "unit": "ns/iter"
          },
          {
            "name": "worker_invocation_tracking/add_with_200_existing",
            "value": 670,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "ws_roundtrip/invoke_echo",
            "value": 139105,
            "range": "± 7220",
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
          "id": "6cde7249bd4f222c17dc0eac5c191947196149bf",
          "message": "chore: bump versions for release -- iii(iii/v0.19.4-next.1)",
          "timestamp": "2026-06-16T18:08:33Z",
          "tree_id": "9261a1fc7f724a8695b53d9cc8486ec802ef4432",
          "url": "https://github.com/iii-hq/iii/commit/6cde7249bd4f222c17dc0eac5c191947196149bf"
        },
        "date": 1781634608913,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2662,
            "range": "± 85",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18092,
            "range": "± 107",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 79095,
            "range": "± 397",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 321626,
            "range": "± 1591",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 42749,
            "range": "± 1145",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 122550,
            "range": "± 415",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 90046,
            "range": "± 1966",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 440276,
            "range": "± 12641",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3947858,
            "range": "± 68075",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 908469,
            "range": "± 2781",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2300677,
            "range": "± 27792",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25039530,
            "range": "± 522450",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4673721,
            "range": "± 106845",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2045,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 532869,
            "range": "± 13651",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42857136,
            "range": "± 164837",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47498093,
            "range": "± 814643",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 58793598,
            "range": "± 5544268",
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
          "id": "906ee5a0a348fd01f3d0939597576030a4fbe3ff",
          "message": "chore: bump versions for release -- iii(iii/v0.19.4)",
          "timestamp": "2026-06-16T18:54:11Z",
          "tree_id": "97327b25e7ad3c4223c8b566f619c8cfa610df1f",
          "url": "https://github.com/iii-hq/iii/commit/906ee5a0a348fd01f3d0939597576030a4fbe3ff"
        },
        "date": 1781637348063,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2586,
            "range": "± 232",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 17847,
            "range": "± 108",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 77458,
            "range": "± 377",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 315238,
            "range": "± 1199",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 43326,
            "range": "± 1475",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 129127,
            "range": "± 746",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 87630,
            "range": "± 1777",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 440895,
            "range": "± 1573",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3830002,
            "range": "± 94743",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 867416,
            "range": "± 5661",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2268525,
            "range": "± 24660",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 22681129,
            "range": "± 1107601",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4355919,
            "range": "± 30849",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2211,
            "range": "± 93",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 513702,
            "range": "± 7683",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42649639,
            "range": "± 302858",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 47054359,
            "range": "± 460918",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57645751,
            "range": "± 2830688",
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
          "id": "55c9653d779da7807a7e80fd7a75f3b26de779e0",
          "message": "chore: bump versions for release -- iii(iii/v0.19.5-next.1)",
          "timestamp": "2026-06-22T16:12:01Z",
          "tree_id": "d346ae038374382121d66bc0915dff3d6eebae71",
          "url": "https://github.com/iii-hq/iii/commit/55c9653d779da7807a7e80fd7a75f3b26de779e0"
        },
        "date": 1782146131453,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2759,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19275,
            "range": "± 218",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 82811,
            "range": "± 727",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 331739,
            "range": "± 1305",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 43563,
            "range": "± 2279",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 134486,
            "range": "± 491",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 85130,
            "range": "± 1248",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 445222,
            "range": "± 4387",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4525728,
            "range": "± 39891",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 857475,
            "range": "± 1335",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2312853,
            "range": "± 39600",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25595117,
            "range": "± 968168",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4378671,
            "range": "± 12072",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2355,
            "range": "± 42",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 513829,
            "range": "± 83108",
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
          "id": "b8aaa570defb3499ae71ab6e02036d4dd608a837",
          "message": "chore: bump versions for release -- iii(iii/v0.19.6-next.1)",
          "timestamp": "2026-06-22T20:51:56Z",
          "tree_id": "d4fac477028e1e9d07fc2e87127058bb751b014b",
          "url": "https://github.com/iii-hq/iii/commit/b8aaa570defb3499ae71ab6e02036d4dd608a837"
        },
        "date": 1782162901174,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2861,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18079,
            "range": "± 70",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 77110,
            "range": "± 275",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 311605,
            "range": "± 2589",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45687,
            "range": "± 1768",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 130232,
            "range": "± 886",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 89162,
            "range": "± 286",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 448939,
            "range": "± 1228",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3754078,
            "range": "± 48131",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 881153,
            "range": "± 3344",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2293225,
            "range": "± 19974",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21350136,
            "range": "± 461892",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4421782,
            "range": "± 35298",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2034,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 528298,
            "range": "± 15531",
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
          "id": "77afe5c3244d0c4a7c416440263148181807a599",
          "message": "chore: bump versions for release -- iii(iii/v0.19.6)",
          "timestamp": "2026-06-23T11:18:10Z",
          "tree_id": "6ad3c8a5bd3d436605c39ce4ec7d628575651c7f",
          "url": "https://github.com/iii-hq/iii/commit/77afe5c3244d0c4a7c416440263148181807a599"
        },
        "date": 1782214798650,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2755,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 19450,
            "range": "± 185",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 82171,
            "range": "± 258",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 332384,
            "range": "± 1134",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 46766,
            "range": "± 1860",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 138989,
            "range": "± 372",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 89137,
            "range": "± 2265",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 456649,
            "range": "± 1770",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 4361640,
            "range": "± 30559",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 884173,
            "range": "± 2602",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2362044,
            "range": "± 14438",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 25640247,
            "range": "± 582208",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4524153,
            "range": "± 34985",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2264,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 509725,
            "range": "± 5174",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42391997,
            "range": "± 465814",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46732391,
            "range": "± 211040",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 56861267,
            "range": "± 4013171",
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
          "id": "23755800a1b5209742620b0cf8a52be994072e5e",
          "message": "chore: bump versions for release -- iii(iii/v0.19.7-next.1)",
          "timestamp": "2026-06-23T22:27:52Z",
          "tree_id": "b975ce5d3bc3bb24b21e29cccb55062df75d6171",
          "url": "https://github.com/iii-hq/iii/commit/23755800a1b5209742620b0cf8a52be994072e5e"
        },
        "date": 1782254990828,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2690,
            "range": "± 98",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18101,
            "range": "± 83",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 78169,
            "range": "± 568",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 321078,
            "range": "± 2043",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 43694,
            "range": "± 1705",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 129003,
            "range": "± 484",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 88291,
            "range": "± 1097",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 433057,
            "range": "± 3569",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3779015,
            "range": "± 114584",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 855650,
            "range": "± 2552",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2240375,
            "range": "± 20353",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21979789,
            "range": "± 858579",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4296135,
            "range": "± 31079",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2071,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 501744,
            "range": "± 12591",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/8",
            "value": 42476261,
            "range": "± 179502",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/32",
            "value": 46786407,
            "range": "± 501322",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/128",
            "value": 57405012,
            "range": "± 9996379",
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
          "id": "847fc9f0cbe34c8c01c236c03c300f9d7c0b3c9b",
          "message": "chore: bump versions for release -- iii(iii/v0.19.7)",
          "timestamp": "2026-06-24T14:36:31Z",
          "tree_id": "b0c8eec671d2e0b9628a39e8c9ca483721a4c407",
          "url": "https://github.com/iii-hq/iii/commit/847fc9f0cbe34c8c01c236c03c300f9d7c0b3c9b"
        },
        "date": 1782313182987,
        "tool": "cargo",
        "benches": [
          {
            "name": "concurrent_invocation/1",
            "value": 2693,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/8",
            "value": 18740,
            "range": "± 553",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/32",
            "value": 81522,
            "range": "± 558",
            "unit": "ns/iter"
          },
          {
            "name": "concurrent_invocation/128",
            "value": 330258,
            "range": "± 1048",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/100",
            "value": 45588,
            "range": "± 1661",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/100",
            "value": 123774,
            "range": "± 721",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/100",
            "value": 86496,
            "range": "± 965",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/1000",
            "value": 452346,
            "range": "± 980",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/1000",
            "value": 3934824,
            "range": "± 43418",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/1000",
            "value": 873270,
            "range": "± 3545",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/functions_register_remove/5000",
            "value": 2336064,
            "range": "± 12448",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/workers_register_unregister/5000",
            "value": 21220717,
            "range": "± 550336",
            "unit": "ns/iter"
          },
          {
            "name": "control_plane_churn/triggers_register_unregister/5000",
            "value": 4428649,
            "range": "± 12659",
            "unit": "ns/iter"
          },
          {
            "name": "core_runtime/engine_call_registered_handler",
            "value": 2046,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "http_concurrency_loopback/1",
            "value": 532888,
            "range": "± 15609",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}