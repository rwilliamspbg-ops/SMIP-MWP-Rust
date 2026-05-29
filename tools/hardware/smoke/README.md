Hardware smoke test for AF_XDP

This tiny tool attempts to create an AF_XDP `RealSocket` against the
interface specified via the `MOHAWK_IFACE` environment variable. It is
meant as a local smoke test on a host with an AF_XDP-capable NIC and the
appropriate privileges.

Usage:

Set environment variables then run the script from the repository root:

```sh
export MOHAWK_IFACE=ens1f0
export MOHAWK_QUEUE_ID=0
# optional: MOHAWK_FRAME_SIZE, MOHAWK_UMEM_PAGES
./tools/hardware/smoke/run_smoke.sh
```

The script will build the smoke binary if necessary and then invoke it.

Do not run this in CI unless you have a dedicated hardware runner.

