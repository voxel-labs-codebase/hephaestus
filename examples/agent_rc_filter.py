"""An agent session against a running Hephaestus: build an RC low-pass filter with an op-amp
buffer, validate it, lay out the board and export manufacturing files.

Run Hephaestus first (``cargo run --release -p hephaestus``), then:

    PYTHONPATH=sdk/python python3 examples/agent_rc_filter.py [--screenshot out.png]

Every change appears live in the application and in its Agents panel.
"""

import argparse
import os
import sys
import tempfile

from hephaestus import Client, HephaestusError


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=7272)
    ap.add_argument("--screenshot")
    args = ap.parse_args()

    pcb = Client(port=args.port, agent="rc-filter-example")
    print("Project:", pcb.get_project_info()["name"])

    # One undoable transaction for the whole circuit.
    pcb.begin_transaction(name="RC filter with buffer")
    try:
        pcb.add_component(part="HDR1X3", designator="J1", value="IN/PWR", position=["1000mil", "4000mil"])
        pcb.add_component(part="RES", designator="R1", value="10k", position=["2000mil", "4000mil"])
        pcb.add_component(part="CAP", designator="C1", value="100nF", position=["2600mil", "3500mil"], rotation=90)
        pcb.add_component(part="OPAMP_SOT23-5", designator="U1", value="LMV321", position=["3800mil", "3900mil"])
        pcb.add_component(part="CAP", designator="C2", value="100nF", position=["3800mil", "4800mil"], rotation=90)
        pcb.add_component(part="HDR1X2", designator="J2", value="OUT", position=["5400mil", "3900mil"])

        pcb.connect_pins(**{"from": {"component": "J1", "pin": "1"}, "to": {"component": "R1", "pin": "1"}})
        pcb.create_net(name="FILT", pins=[{"component": "R1", "pin": "2"}, {"component": "C1", "pin": "2"}, {"component": "U1", "pin": "+"}])
        pcb.create_net(name="OUT", pins=[{"component": "U1", "pin": "OUT"}, {"component": "U1", "pin": "-"}, {"component": "J2", "pin": "1"}])
        pcb.create_net(name="VCC", pins=[{"component": "J1", "pin": "2"}, {"component": "U1", "pin": "V+"}, {"component": "C2", "pin": "2"}])
        pcb.create_net(name="GND", pins=[{"component": "J1", "pin": "3"}, {"component": "C1", "pin": "1"}, {"component": "U1", "pin": "V-"}, {"component": "C2", "pin": "1"}, {"component": "J2", "pin": "2"}])
        pcb.commit_transaction()
    except HephaestusError as e:
        print("failed, rolling back:", e)
        pcb.rollback_transaction()
        return 1

    nets = pcb.get_nets()
    for n in nets["nets"]:
        print(f"  {n['name']:8} {', '.join(n['pins'])}")
    erc = pcb.run_erc()
    print(f"ERC: {erc['errors']} errors, {erc['warnings']} warnings")

    pcb.update_pcb()
    placements = {"J1": (5, 12.5, 0), "R1": (14, 15, 0), "C1": (14, 9, 90), "U1": (24, 12.5, 0), "C2": (24, 20, 0), "J2": (35, 12.5, 0)}
    for des, (x, y, rot) in placements.items():
        pcb.place_component(component=des, position=[x, y], rotation=rot)
    for net in ["FILT", "OUT"]:
        try:
            r = pcb.route_net(net=net, require_clean=True)
            print(f"routed {net}: {r['status']}")
        except HephaestusError as e:
            print(f"left {net} for interactive routing: {e.message}")
    board = pcb.get_pcb()
    print("Routing:", board["routing"])
    drc = pcb.run_drc()
    print(f"DRC: {drc['errors']} errors ({sum(1 for v in drc['violations'] if v['rule'] == 'Un-Routed Net')} unrouted connections)")

    out = os.path.join(tempfile.gettempdir(), "hephaestus-rc-filter")
    files = pcb.export_gerber(directory=out)["files"] + pcb.export_drill_files(directory=out)["files"]
    print(f"Wrote {len(files)} fabrication files to {out}")
    print("BOM:", [(r["Designator"], r["Comment"]) for r in pcb.export_bom()["rows"]])

    if args.screenshot:
        pcb.open_document("pcb")
        print("Screenshot:", pcb.screenshot(args.screenshot)["path"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
