setup_python:
	python -m venv .venv && source .venv/bin/activate && \
	pip install -r requirements.txt 

generate_python:
	python -m grpc_tools.protoc -I proto --python_out=worker_py --grpc_python_out=worker_py proto/engine.proto

run_stream_server:
	source .venv/bin/activate && python worker_py/stream_server.py

run_worker_server:
	source .venv/bin/activate && python worker_py/worker_server.py

run_socket_server:
	cargo run --bin server

run_socket_client:
	cargo run --bin client
