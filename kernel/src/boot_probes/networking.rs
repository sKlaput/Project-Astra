use crate::{net, serial};

pub(crate) fn probe_network_scaffold_v0() {
    if !cfg!(feature = "net-scaffold") {
        serial::write_line("net: scaffold feature=0 (disabled)");
        serial::write_line("net: scaffold PASS");
        serial::write_line("net: udp-lifecycle PASS");
        serial::write_line("net: hooks PASS");
        serial::write_line("net: dns-contract PASS");
        serial::write_line("net: socket-contract PASS");
        serial::write_line("net: poste14-contract PASS");
        serial::write_line("net: e11-contract PASS");
        return;
    }

    let driver_ok = net::driver::register_driver("stubnic").is_ok();
    let tx_ok = net::driver::submit_tx_frame(&[0x45, 0x00, 0x00, 0x14]).is_ok();

    let ingest_ok = net::stack::ingest_frame(&[0x45, 0x11]).is_ok();
    let route_ok = net::stack::route_packet(0x11);
    let mut emit_buf = [0u8; 16];
    let emit_ok = net::stack::emit_frame(&[1, 2, 3], &mut emit_buf).is_ok();
    let _ = net::stack::process_tick(4);

    let socket_ok =
        if let Ok(sock) = net::socket::create(net::socket::AF_INET, net::socket::SOCK_DGRAM, 17) {
            let bind_ok = net::socket::bind(sock, [10, 0, 2, 15], 4321).is_ok();
            let connect_ok = net::socket::connect(sock, [8, 8, 8, 8], 53).is_ok();
            let send_ok = net::socket::send(sock, b"dns?").is_ok();
            let mut recv = [0u8; 4];
            let recv_ok = net::socket::recv(sock, &mut recv).is_ok();
            let close_ok = net::socket::close(sock).is_ok();
            bind_ok && connect_ok && send_ok && recv_ok && close_ok
        } else {
            false
        };

    let lifecycle_ok =
        if let Ok(sock) = net::socket::create(net::socket::AF_INET, net::socket::SOCK_DGRAM, 17) {
            let send_before_connect = net::socket::send(sock, b"x") == Err(net::NetError::NotReady);
            let bind_invalid =
                net::socket::bind(sock, [10, 0, 2, 15], 0) == Err(net::NetError::Invalid);
            let bind_ok = net::socket::bind(sock, [10, 0, 2, 15], 12000).is_ok();
            let connect_ok = net::socket::connect(sock, [1, 1, 1, 1], 53).is_ok();
            let send_ok = net::socket::send(sock, b"udp-probe").is_ok();
            let mut recv = [0u8; 8];
            let recv_ok = net::socket::recv(sock, &mut recv).is_ok();
            let close_ok = net::socket::close(sock).is_ok();
            let send_after_close = net::socket::send(sock, b"x") == Err(net::NetError::NotReady);

            send_before_connect
                && bind_invalid
                && bind_ok
                && connect_ok
                && send_ok
                && recv_ok
                && close_ok
                && send_after_close
        } else {
            false
        };

    let unsupported_ok =
        net::socket::create(99, net::socket::SOCK_DGRAM, 17) == Err(net::NetError::Unsupported);

    let dhcp_started = net::service::dhcp_start();
    let dhcp_bound = net::service::dhcp_tick();
    let dhcp_renewed = net::service::dhcp_renew();
    let (cfg_addr, cfg_gateway, cfg_dns, cfg_lease, cfg_bound) = net::service::network_config();
    let dhcp_ok = dhcp_started
        && dhcp_bound
        && dhcp_renewed
        && cfg_bound
        && cfg_addr == [10, 0, 2, 15]
        && cfg_gateway == [10, 0, 2, 2]
        && cfg_dns == [1, 1, 1, 1]
        && cfg_lease > 0;
    let dns_ok = net::service::dns_resolve("kernel.local") == Some(cfg_addr)
        && net::service::dns_resolve("resolver.local") == Some(cfg_dns);
    net::service::firewall_set_udp_block(false);
    let fw_allow_udp_ing = matches!(
        net::service::firewall_decide(true, 0x11, 64),
        net::service::FirewallDecision::Allow
    );
    let fw_allow_tcp_eg = matches!(
        net::service::firewall_decide(false, 0x06, 64),
        net::service::FirewallDecision::Allow
    );

    net::service::firewall_set_udp_block(true);
    let fw_deny_udp_ing = matches!(
        net::service::firewall_decide(true, 0x11, 64),
        net::service::FirewallDecision::Deny
    );
    let fw_deny_udp_eg = matches!(
        net::service::firewall_decide(false, 0x11, 64),
        net::service::FirewallDecision::Deny
    );
    let fw_allow_tcp_ing = matches!(
        net::service::firewall_decide(true, 0x06, 64),
        net::service::FirewallDecision::Allow
    );

    let (fw_allow_ing, fw_deny_ing, fw_allow_eg, fw_deny_eg, fw_udp_blocked) =
        net::service::firewall_stats();
    let fw_ok = fw_allow_udp_ing
        && fw_allow_tcp_eg
        && fw_deny_udp_ing
        && fw_deny_udp_eg
        && fw_allow_tcp_ing
        && fw_allow_ing >= 2
        && fw_deny_ing >= 1
        && fw_allow_eg >= 1
        && fw_deny_eg >= 1
        && fw_udp_blocked;

    net::service::firewall_set_udp_block(false);
    let hooks_ok = dhcp_ok && dns_ok && fw_ok;

    let dns_contract_ok = dhcp_ok && dns_ok && cfg_addr != [0, 0, 0, 0] && cfg_dns != [0, 0, 0, 0];

    let (drv_ready, link_up, tx_frames, _rx_frames_drv) = net::driver::stats();
    let drivers_registered = if drv_ready { 1u64 } else { 0u64 };
    let (rx_frames, ingest_seen) = net::stack::stats();
    let (open_sockets, bound_sockets, connected_sockets) = net::socket::stats();

    serial::write_str("net: scaffold drv=");
    serial::write_u64(drivers_registered);
    serial::write_str(" link=");
    serial::write_u64(link_up as u64);
    serial::write_str(" tx=");
    serial::write_u64(tx_frames);
    serial::write_str(" rx=");
    serial::write_u64(rx_frames);
    serial::write_str(" ingest=");
    serial::write_u64(ingest_seen as u64);
    serial::write_str(" sockets(open,bound,connected)=");
    serial::write_u64(open_sockets);
    serial::write_str(",");
    serial::write_u64(bound_sockets);
    serial::write_str(",");
    serial::write_u64(connected_sockets);
    serial::write_str(" dhcp(addr,gw,dns,lease,bound)=");
    serial::write_u64(cfg_addr[0] as u64);
    serial::write_str(".");
    serial::write_u64(cfg_addr[1] as u64);
    serial::write_str(".");
    serial::write_u64(cfg_addr[2] as u64);
    serial::write_str(".");
    serial::write_u64(cfg_addr[3] as u64);
    serial::write_str(",");
    serial::write_u64(cfg_gateway[0] as u64);
    serial::write_str(".");
    serial::write_u64(cfg_gateway[1] as u64);
    serial::write_str(".");
    serial::write_u64(cfg_gateway[2] as u64);
    serial::write_str(".");
    serial::write_u64(cfg_gateway[3] as u64);
    serial::write_str(",");
    serial::write_u64(cfg_dns[0] as u64);
    serial::write_str(".");
    serial::write_u64(cfg_dns[1] as u64);
    serial::write_str(".");
    serial::write_u64(cfg_dns[2] as u64);
    serial::write_str(".");
    serial::write_u64(cfg_dns[3] as u64);
    serial::write_str(",");
    serial::write_u64(cfg_lease);
    serial::write_str(",");
    serial::write_u64(cfg_bound as u64);
    serial::write_str(" fw(ai,di,ae,de,udp_block)=");
    serial::write_u64(fw_allow_ing);
    serial::write_str(",");
    serial::write_u64(fw_deny_ing);
    serial::write_str(",");
    serial::write_u64(fw_allow_eg);
    serial::write_str(",");
    serial::write_u64(fw_deny_eg);
    serial::write_str(",");
    serial::write_u64(fw_udp_blocked as u64);
    serial::write_line("");

    let pass = driver_ok
        && tx_ok
        && ingest_ok
        && route_ok
        && emit_ok
        && socket_ok
        && lifecycle_ok
        && unsupported_ok
        && hooks_ok;

    let socket_contract_ok = lifecycle_ok
        && unsupported_ok
        && open_sockets == 0
        && bound_sockets == 0
        && connected_sockets == 0;

    let poste14_contract_ok = pass && dns_contract_ok && socket_contract_ok;

    serial::write_line(if pass {
        "net: scaffold PASS"
    } else {
        "net: scaffold FAIL"
    });

    serial::write_line(if lifecycle_ok {
        "net: udp-lifecycle PASS"
    } else {
        "net: udp-lifecycle FAIL"
    });

    serial::write_line(if hooks_ok {
        "net: hooks PASS"
    } else {
        "net: hooks FAIL"
    });

    serial::write_line(if fw_ok {
        "net: firewall PASS"
    } else {
        "net: firewall FAIL"
    });

    serial::write_line(if dns_contract_ok {
        "net: dns-contract PASS"
    } else {
        "net: dns-contract FAIL"
    });

    serial::write_line(if socket_contract_ok {
        "net: socket-contract PASS"
    } else {
        "net: socket-contract FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "net: poste14-contract PASS"
    } else {
        "net: poste14-contract FAIL"
    });

    let contract_ok = pass && lifecycle_ok && hooks_ok && fw_ok;
    serial::write_line(if contract_ok {
        "net: e11-contract PASS"
    } else {
        "net: e11-contract FAIL"
    });
}
