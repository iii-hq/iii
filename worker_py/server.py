import os
from concurrent import futures

import json

import engine_pb2
import engine_pb2_grpc
import grpc
from google.protobuf import json_format, struct_pb2


def to_value(data):
    return json_format.Parse(json.dumps(data), struct_pb2.Value())


class WorkerServicer(engine_pb2_grpc.WorkerServicer):
    def Process(self, request, context):
        method = request.method or "format_text"

        if method == "format_text":
            print(
                f"Received request for service={request.service} method={method} payload={request.payload}"
            )
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
