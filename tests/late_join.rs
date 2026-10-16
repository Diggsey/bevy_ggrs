use bevy::{platform::collections::HashMap, prelude::*, time::TimeUpdateStrategy};
use bevy_ggrs::{
    GgrsConfig, GgrsPlugin, GgrsSchedule, LocalInputs, LocalPlayers, ReadInputs,
    RollbackFrameCount, Session,
};
use core::time::Duration;
use ggrs::{P2PSession, PlayerType, SessionBuilder, SessionState, UdpNonBlockingSocket};
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

type TestConfig = GgrsConfig<u8>;

fn addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn read_local_inputs(mut commands: Commands, local_players: Res<LocalPlayers>) {
    let inputs: HashMap<_, _> = local_players.0.iter().map(|&h| (h, 1u8)).collect();
    commands.insert_resource(LocalInputs::<TestConfig>(inputs));
}

fn app(session: P2PSession<TestConfig>) -> App {
    let start = session.start_frame();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(GgrsPlugin::<TestConfig>::default())
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .add_systems(ReadInputs, read_local_inputs)
        .add_systems(GgrsSchedule, || {});
    app.insert_resource(Session::P2P(session));
    app.insert_resource(RollbackFrameCount(start));
    app
}

fn frame(app: &App) -> i32 {
    match app.world().resource::<Session<TestConfig>>() {
        Session::P2P(s) => s.current_frame(),
        _ => unreachable!(),
    }
}

fn session(app: &mut App) -> Mut<'_, P2PSession<TestConfig>> {
    app.world_mut()
        .resource_mut::<Session<TestConfig>>()
        .map_unchanged(|s| match s {
            Session::P2P(s) => s,
            _ => unreachable!(),
        })
}

/// A peer that joins a running session late runs extra frames each update until it has caught
/// up with the others, rather than staying as far behind as it started.
#[test]
#[serial]
fn a_late_joiner_catches_up() -> Result<(), Box<dyn std::error::Error>> {
    let host = SessionBuilder::<TestConfig>::new()
        .with_late_join_window(600)
        .add_player(PlayerType::Local, 0)?
        .add_open_slot(1)?
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(8191)?)?;
    let mut host = app(host);
    for _ in 0..200 {
        host.update();
    }
    // the joiner loads a state from a while ago
    let start = session(&mut host).confirmed_frame() - 180;
    session(&mut host).add_remote_player(1, addr(8192), start)?;

    let joiner = SessionBuilder::<TestConfig>::new()
        .with_start_frame(start)
        .add_player(PlayerType::Remote(addr(8191)), 0)?
        .add_joining_local_player(1)?
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(8192)?)?;
    let mut joiner = app(joiner);
    assert!(frame(&host) - start > 150);

    for _ in 0..60 {
        host.update();
        joiner.update();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(session(&mut joiner).current_state(), SessionState::Running);
    let behind = frame(&host) - frame(&joiner);
    assert!(behind <= 4, "the joiner is {behind} frames behind");
    assert!(session(&mut joiner).frames_behind() <= 4);
    Ok(())
}
