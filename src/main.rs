#![feature(async_await)]
#![deny(
    missing_debug_implementations,
    missing_copy_implementations,
    elided_lifetimes_in_paths,
    rust_2018_idioms,
    clippy::fallible_impl_from,
    clippy::missing_const_for_fn,
    intra_doc_link_resolution_failure
)]

use std::{
    io::{self, Result, Write},
    process::{Command, Output},
};

const ROOT_HANDLE: &str = "1:0";
const ROOT_VIP_HANDLE: &str = "1:1";
const CLASS_ID: &str = "1:10";
const CLASS_VIP_ID: &str = "1:5";
const DEFAULT_RATE: &str = "120kbps";
const VIP_RATE: &str = "256kbps";
const VIP_IP_LIST: [&str; 1] = ["192.168.1.111"];
const INTERFACE: &str = "wlan0";

fn main() -> Result<()> {
    // tc is the program that is in charge of setting up the shaping rules.
    setup_tc(INTERFACE)?;
    // Once that class is setup, we’ll need to setup iptables to mark the
    // specific packets we want to shape as such.
    setup_iptables(INTERFACE)?;

    println!("All DONE");
    Ok(())
}

fn setup_tc(interface: &str) -> Result<()> {
    // a nice guide http://sirlagz.net/2013/01/27/how-to-turn-the-raspberry-pi-into-a-shaping-wifi-router/

    // **Step 1**
    //
    // Firstly, we will setup the default rule for the interface, which is
    // {interface} in this instance.
    //
    // These 2 commands sets the default policy on {interface} to shape
    // everyone’s download speed to {DEFAULT_RATE} kilobytes a second.

    let cmd = format!(
        "tc qdisc add dev {device} root handle {root_handle} htb default 10",
        device = interface,
        root_handle = ROOT_HANDLE
    );
    let output = run_as_sudo(&cmd)?;
    assert!(output.status.success(), "Error while adding QDISC");

    let cmd = format!(
        "tc class add dev {device} parent {root_handle} classid {class_id} htb rate {default_rate} \
        ceil \
        {default_rate} \
        prio 0",
        device = interface,
        root_handle = ROOT_HANDLE,
        class_id = CLASS_ID,
        default_rate = DEFAULT_RATE,
    );
    let output = run_as_sudo(&cmd)?;
    assert!(output.status.success(), "Error while adding CLASS");

    // **Step 2**
    //
    // Next, we’ll setup another class to shape certain addresses to a higher
    // speed. We also need to setup a filter so that any packets marked as
    // such go through this rule

    let cmd = format!(
        "tc class add dev {device} parent {root_handle} classid {class_id} htb rate {default_rate} \
        ceil \
        {default_rate} \
        prio 1",
        device = interface,
        root_handle = ROOT_VIP_HANDLE,
        class_id = CLASS_VIP_ID,
        default_rate = VIP_RATE,
    );
    let output = run_as_sudo(&cmd)?;
    assert!(output.status.success(), "Error while adding CLASS VIP");

    let cmd = format!(
        "tc filter add dev {device} parent {root_handle} prio 1 handle 5 fw flowid {class_id}",
        device = interface,
        root_handle = ROOT_VIP_HANDLE,
        class_id = CLASS_VIP_ID,
    );
    let output = run_as_sudo(&cmd)?;
    assert!(output.status.success(), "Error while adding FILTER VIP");

    Ok(())
}

fn setup_iptables(interface: &str) -> Result<()> {
    // **Step 1**
    // Firstly, we’ll create the mangle table that we need.
    // I’ve used a custom chain in the mangle table in this snippet The below
    // code creates the new chains of `shaper-in` and `shaper-out`, and then
    // sets up some rules for any packets coming in and out of {interface}
    // to go through the new chains.

    let output = run_as_sudo("iptables -t mangle -N shaper-out")?;
    assert!(output.status.success(), "Error while adding shaper-out");

    let output = run_as_sudo("iptables -t mangle -N shaper-in")?;
    assert!(output.status.success(), "Error while adding shaper-in");

    let cmd = format!(
        "iptables -t mangle -I POSTROUTING -o {} -j shaper-in",
        interface
    );
    let output = run_as_sudo(&cmd)?;
    assert!(output.status.success(), "Error while adding POSTROUTING");

    let cmd = format!(
        "iptables -t mangle -I PREROUTING -i {} -j shaper-out",
        interface
    );
    let output = run_as_sudo(&cmd)?;
    assert!(output.status.success(), "Error while adding PREROUTING");

    // **Step 2**
    //
    // Once that is done, we can then setup the packet marking so that any
    // packets from the 192.168.1.0/24 subnet gets marked with a 1, otherwise if
    // the IP address is in {VIP_IP}, they will get marked with a 5

    let output = run_as_sudo("iptables -t mangle -A shaper-out -s 192.168.1.0/24 -j MARK --set-mark 1")?;
    assert!(output.status.success(), "Error while marking out 1");

    let output = run_as_sudo("iptables -t mangle -A shaper-in -d 192.168.1.0/24 -j MARK --set-mark 1")?;
    assert!(output.status.success(), "Error while marking 1");

    for ip in &VIP_IP_LIST {
        let output = run_as_sudo(&format!(
            "iptables -t mangle -A shaper-out -s {} -j MARK --set-mark 5",
            ip
        ))?;
        assert!(
            output.status.success(),
            "Error while marking out 5 for ip = {}",
            ip
        );

        let output = run_as_sudo(&format!(
            "iptables -t mangle -A shaper-in -d {} -j MARK --set-mark 5",
            ip
        ))?;
        assert!(
            output.status.success(),
            "Error while marking in 5 for ip = {}",
            ip
        );
    }
    Ok(())
}

fn run_as_sudo(cmd: &str) -> Result<Output> {
    let output = Command::new("sudo")
        .arg("sh")
        .args(&["-c", &cmd])
        .output()
        .expect("command not found");
    io::stdout().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    Ok(output)
}
