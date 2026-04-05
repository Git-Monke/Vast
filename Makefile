publish:
	spacetime publish --server http://localhost:3000 --anonymous vast

build:
	spacetime build

explore:
	cargo run -p explorer --release

.ONESHELL:

reset-db:
	pkill spacetime
	sleep 1
	rm -rf /home/monke/.local/share/spacetime/data
	spacetime start &
	sleep 2
	spacetime publish --server http://localhost:3000
