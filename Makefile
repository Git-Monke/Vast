publish:
	spacetime publish --server http://localhost:3000 --anonymous vast

build:
	spacetime build

explore:
	cargo run -p explorer --release
