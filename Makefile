
generate_python:
	python -m grpc_tools.protoc -I proto --python_out=worker_py --grpc_python_out=worker_py proto/engine.proto

