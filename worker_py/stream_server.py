from concurrent import futures
import json
import os
import time
from typing import Iterator

import engine_pb2
import engine_pb2_grpc
import grpc
from google.protobuf import json_format, struct_pb2


def chunk_text(value: str, chunk_size: int) -> Iterator[str]:
    if chunk_size <= 0:
        yield value
        return

    for index in range(0, len(value), chunk_size):
        yield value[index : index + chunk_size]


def to_value(data):
    return json_format.Parse(json.dumps(data), struct_pb2.Value())


class StreamWorkerServicer(engine_pb2_grpc.WorkerServicer):
    def Process(self, request, context):
        method = request.method or ""

        if method == "service_registered":
            meta = dict(request.meta)
            new_service = meta.get("new_service_name", "unknown service")
            new_address = meta.get("new_service_address", "unknown address")
            new_type = meta.get("new_service_type", "unspecified")
            print(
                "[streamer] received new service notification:",
                f"service={new_service}",
                f"address={new_address}",
                f"type={new_type}",
                f"methods={meta.get('new_service_methods', '')}",
            )
            return engine_pb2.ProcessResponse(result="ack")

        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details("use StreamProcess for this service")
        return engine_pb2.ProcessResponse()

    def StreamProcess(self, request, context):
        method = request.method or "stream_chunks"
        if method != "stream_chunks":
            context.set_code(grpc.StatusCode.UNIMPLEMENTED)
            context.set_details(f"method '{method}' is not implemented")
            return

        meta = dict(request.meta)
        delay_ms = int(meta.get("delay_ms", "0") or 0)
        chunk_size = int(meta.get("chunk_size", "8") or 8)
        payload = request.payload or ""

        print(
            f"Streaming {len(payload)} bytes for service={request.service} method={method}"
        )

        for chunk in chunk_text(payload, chunk_size):
            if delay_ms > 0:
                time.sleep(delay_ms / 1000.0)
            yield engine_pb2.ProcessResponse(result=chunk)


def register_with_engine():
    engine_addr = os.environ.get("ENGINE_ADDR", "localhost:50051")
    service_name = os.environ.get("SERVICE_NAME", "text-streamer")
    service_addr = os.environ.get("SERVICE_ADDR", "http://127.0.0.1:50053")

    channel = grpc.insecure_channel(engine_addr)
    stub = engine_pb2_grpc.EngineStub(channel)
    request = engine_pb2.RegisterServiceRequest(
        name=service_name,
        address=service_addr,
        service_type="python",
        methods=[
            engine_pb2.MethodDescriptor(
                name="stream_chunks",
                description="Emit payload in fixed-size chunks",
                kind=engine_pb2.METHOD_KIND_SERVER_STREAMING,
                request_format=to_value(
                    {
                        "payload": {"type": "string"},
                        "meta": {
                            "chunk_size": {"type": "integer", "optional": True},
                            "delay_ms": {"type": "integer", "optional": True},
                        },
                    }
                ),
                response_format=to_value(
                    {
                        "stream": {
                            "result": {"type": "string", "description": "chunk"}
                        }
                    }
                ),
            ),
            engine_pb2.MethodDescriptor(
                name="service_registered",
                description="Receive notifications about new services",
                kind=engine_pb2.METHOD_KIND_UNARY,
                request_format=to_value(
                    {
                        "meta": {
                            "event": "service_registered",
                            "registration_kind": {"type": "string"},
                            "new_service_name": {"type": "string"},
                            "new_service_address": {"type": "string"},
                            "new_service_type": {"type": "string", "optional": True},
                            "new_service_methods": {"type": "string", "optional": True},
                        }
                    }
                ),
                response_format=to_value({"result": "ack"}),
            ),
        ],
    )
    response = stub.RegisterService(request, timeout=5)
    print(f"Registered '{service_name}' at {service_addr}: {response.message}")


def serve():
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=8))
    engine_pb2_grpc.add_WorkerServicer_to_server(StreamWorkerServicer(), server)
    server.add_insecure_port("0.0.0.0:50053")
    server.start()
    print("Streaming worker listening on 0.0.0.0:50053")

    try:
        register_with_engine()
    except grpc.RpcError as exc:
        print(f"Failed to register streaming service with engine: {exc}")

    server.wait_for_termination()


if __name__ == "__main__":
    serve()
