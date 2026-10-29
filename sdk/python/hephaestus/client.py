"""A dependency-free client for the Hephaestus control protocol."""

from __future__ import annotations

import json
import os
import socket
from typing import Any


class HephaestusError(Exception):
    """A structured error returned by Hephaestus (``code``, ``message``, ``details``)."""

    def __init__(self, code: str, message: str, details: Any = None):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message
        self.details = details


class Client:
    """Call Hephaestus agent tools.

    Every tool of ``list_tools()`` is also available as a method: ``client.get_nets(net="GND")``
    is ``client.call("get_nets", {"net": "GND"})``. Lengths are millimetres (numbers) or
    strings with units such as ``"100mil"``.
    """

    def __init__(self, host: str = "127.0.0.1", port: int = 7272, agent: str = "python", timeout: float = 120.0):
        self.agent = agent
        self.token = os.environ.get("HEPHAESTUS_TOKEN") or os.environ.get("PCBCRAFT_TOKEN")
        self._sock = socket.create_connection((host, port), timeout=timeout)
        self._file = self._sock.makefile("rwb")
        self._next_id = 1

    def close(self) -> None:
        self._file.close()
        self._sock.close()

    def __enter__(self) -> "Client":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def call(self, tool: str, args: dict[str, Any] | None = None) -> Any:
        request: dict[str, Any] = {"id": self._next_id, "tool": tool, "args": args or {}, "agent": self.agent}
        if self.token:
            request["token"] = self.token
        self._next_id += 1
        self._file.write((json.dumps(request) + "\n").encode())
        self._file.flush()
        line = self._file.readline()
        if not line:
            raise HephaestusError("connection", "Hephaestus closed the connection")
        reply = json.loads(line)
        if reply.get("ok"):
            return reply.get("result")
        err = reply.get("error") or {}
        raise HephaestusError(err.get("code", "error"), err.get("message", "unknown error"), err.get("details"))

    def list_tools(self) -> list[dict[str, Any]]:
        return self.call("list_tools")

    def screenshot(self, path: str) -> Any:
        """Save a PNG of the application window (GUI sessions only; absolute path)."""
        return self.call("ui.screenshot", {"path": os.path.abspath(path)})

    def open_document(self, document: str) -> Any:
        """Show a document in the GUI: ``"pcb"`` or a sheet name."""
        return self.call("ui.open_document", {"document": document})

    def __getattr__(self, name: str):
        if name.startswith("_"):
            raise AttributeError(name)

        def tool(**kwargs: Any) -> Any:
            return self.call(name, kwargs)

        tool.__name__ = name
        return tool
