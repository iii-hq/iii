import json
import os
import time
from concurrent import futures

import engine_pb2
import engine_pb2_grpc
import grpc
from google.protobuf import json_format, struct_pb2


def to_value(data):
    return json_format.Parse(json.dumps(data), struct_pb2.Value())


def chunk_text(value: str, chunk_size: int):
    if chunk_size <= 0:
        yield value
        return

    for index in range(0, len(value), chunk_size):
        yield value[index : index + chunk_size]


class WorkerServicer(engine_pb2_grpc.WorkerServicer):
    def Process(self, request, context):
        method = request.method or "format_text"

        if method == "format_text":
            print(f"Received request for service={request.service} method={method} payload={request.payload}")
            meta = dict(request.meta)
            prefix = meta.get("prefix", "")
            suffix = meta.get("suffix", "")
            result = f"{prefix}{request.payload.upper()}{suffix}"
            return engine_pb2.ProcessResponse(result=result)

        if method == "service_registered":
            meta = dict(request.meta)
            new_service = meta.get("new_service_name", "unknown service")
            new_address = meta.get("new_service_address", "unknown address")
            new_type = meta.get("new_service_type", "unspecified")
            print(
                "Received service registration notification:",
                f"service={new_service}",
                f"address={new_address}",
                f"type={new_type}",
                f"methods={meta.get('new_service_methods', '')}",
            )
            return engine_pb2.ProcessResponse(result="ack")

        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details(f"method '{method}' is not implemented")
        return engine_pb2.ProcessResponse()

    def StreamProcess(self, request, context):
        method = request.method or ""

        if method != "stream_format":
            context.set_code(grpc.StatusCode.UNIMPLEMENTED)
            context.set_details(f"method '{method}' is not implemented")
            return

        meta = dict(request.meta)
        delay_ms = int(meta.get("delay_ms", "0") or 0)
        chunk_size = int(meta.get("chunk_size", "8") or 8)
        prefix = meta.get("prefix", "")
        suffix = meta.get("suffix", "")
        payload = request.payload or ""
        formatted = f"{prefix}{payload.upper()}{suffix}"

        print(
            "Streaming formatted payload:",
            f"service={request.service}",
            f"method={method}",
            f"chunk_size={chunk_size}",
            f"delay_ms={delay_ms}",
        )

        for chunk in chunk_text(formatted, chunk_size):
            if delay_ms > 0:
                time.sleep(delay_ms / 1000.0)
            yield engine_pb2.ProcessResponse(result=chunk)


def register_with_engine():
    engine_addr = os.environ.get("ENGINE_ADDR", "localhost:50051")
    service_name = os.environ.get("SERVICE_NAME", "text-formatter")
    service_addr = os.environ.get("SERVICE_ADDR", "http://127.0.0.1:50052")

    channel = grpc.insecure_channel(engine_addr)
    stub = engine_pb2_grpc.EngineStub(channel)
    request = engine_pb2.RegisterServiceRequest(
        name=service_name,
        address=service_addr,
        service_type="python",
        methods=[
            engine_pb2.MethodDescriptor(
                name="format_text",
                description="Uppercase text with optional prefix/suffix",
                kind=engine_pb2.METHOD_KIND_UNARY,
                request_format=to_value(
                    {
                        "payload": {"type": "string"},
                        "meta": {
                            "prefix": {"type": "string", "optional": True},
                            "suffix": {"type": "string", "optional": True},
                        },
                    }
                ),
                response_format=to_value({"result": {"type": "string"}}),
            ),
            engine_pb2.MethodDescriptor(
                name="stream_format",
                description="Uppercase payload and stream back fixed-size chunks",
                kind=engine_pb2.METHOD_KIND_SERVER_STREAMING,
                request_format=to_value(
                    {
                        "payload": {"type": "string"},
                        "meta": {
                            "prefix": {"type": "string", "optional": True},
                            "suffix": {"type": "string", "optional": True},
                            "chunk_size": {"type": "integer", "optional": True},
                            "delay_ms": {"type": "integer", "optional": True},
                        },
                    }
                ),
                response_format=to_value(
                    {
                        "stream": {
                            "result": {
                                "type": "string",
                                "description": "uppercased chunk with optional prefix/suffix",
                            }
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

    register_api_service(stub, service_name, service_addr)


def register_api_service(stub, base_service_name, service_addr):
    api_name = os.environ.get("SERVICE_API_NAME") or f"{base_service_name}-api"
    http_method = os.environ.get("SERVICE_API_METHOD", "POST").upper()
    method_name = "format_text"
    default_path = f"/{base_service_name}/{method_name}"
    api_path = os.environ.get("SERVICE_API_PATH") or default_path

    request = engine_pb2.RegisterServiceRequest(
        name=api_name,
        address=service_addr,
        service_type="api",
        methods=[
            engine_pb2.MethodDescriptor(
                name=method_name,
                description="Uppercase text via HTTP",
                kind=engine_pb2.METHOD_KIND_UNARY,
                request_format=to_value(
                    {
                        "http": {"method": http_method, "path": api_path},
                        "payload": {"type": "string"},
                        "meta": {
                            "prefix": {"type": "string", "optional": True},
                            "suffix": {"type": "string", "optional": True},
                        },
                    }
                ),
                response_format=to_value({"result": {"type": "string"}}),
            )
        ],
    )

    try:
        response = stub.RegisterService(request, timeout=5)
        print(
            "Registered API service:",
            f"name={api_name}",
            f"path={api_path}",
            f"method={http_method}",
            f"message={response.message}",
        )
    except grpc.RpcError as exc:
        print(f"Failed to register API service '{api_name}': {exc}")


def serve():
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=8))
    engine_pb2_grpc.add_WorkerServicer_to_server(WorkerServicer(), server)
    server.add_insecure_port("0.0.0.0:50052")
    server.start()
    print("Worker listening on 0.0.0.0:50052")

    try:
        register_with_engine()
    except grpc.RpcError as exc:
        print(f"Failed to register service with engine: {exc}")

    server.wait_for_termination()


if __name__ == "__main__":
    serve()
