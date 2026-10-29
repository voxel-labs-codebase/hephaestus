"""Python client for Hephaestus.

Connects to a running Hephaestus (or ``hephaestus-mcp``-independent headless server) over the
local control protocol: newline-delimited JSON on 127.0.0.1 (default port 7272).

    from hephaestus import Client
    pcb = Client()
    pcb.add_component(part="RES", position=[25.4, 25.4])
    print(pcb.get_nets())
"""

from .client import Client, HephaestusError

__all__ = ["Client", "HephaestusError"]
__version__ = "0.1.0"
