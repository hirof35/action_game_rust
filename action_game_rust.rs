use bevy::prelude::*;
use bevy_rapier2d::prelude::*;

// --- 定数・列挙型 ---
#[derive(States, Debug, Clone, Eq, PartialEq, Hash, Default)]
enum GameState {
    #[default]
    Title,
    Playing,
    GameOver,
    GameClear,
}

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Enemy {
    speed: f32,
    direction: f32,
}

#[derive(Component)]
struct Boss;

#[derive(Component)]
struct Health(i32);

#[derive(Component)]
struct VisualElement; // UI消去用のタグ

#[derive(Resource, Default)]
struct GameProgress {
    enemies_defeated: u32,
    boss_spawned: bool,
    boss_defeated: bool,
}

// --- メイン関数 ---
fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(100.0))
        .add_plugins(RapierDebugRenderPlugin::default())
        .init_state::<GameState>()
        .init_resource::<GameProgress>()
        
        // タイトル
        .add_systems(OnEnter(GameState::Title), setup_title)
        .add_systems(Update, start_game.run_if(in_state(GameState::Title)))
        .add_systems(OnExit(GameState::Title), cleanup_ui)

        // ゲーム本編
        .add_systems(OnEnter(GameState::Playing), setup_game)
        .add_systems(Update, (
            player_control,
            enemy_ai,
            collision_logic,
            boss_system,
            camera_follow,
            check_end_conditions,
        ).run_if(in_state(GameState::Playing)))
        .add_systems(OnExit(GameState::Playing), cleanup_all)

        // ゲームオーバー・クリア
        .add_systems(OnEnter(GameState::GameOver), setup_game_over)
        .add_systems(Update, back_to_title.run_if(in_state(GameState::GameOver).or_else(in_state(GameState::GameClear))))
        .add_systems(OnEnter(GameState::GameClear), setup_game_clear)
        .add_systems(OnExit(GameState::GameOver), cleanup_ui)
        .add_systems(OnExit(GameState::GameClear), cleanup_ui)

        .run();
}

// --- システム実装 ---

fn setup_title(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
    spawn_ui(&mut commands, "RUST ACTION\n\nPress SPACE to Start", Color::CYAN);
}

fn setup_game(mut commands: Commands, mut progress: ResMut<GameProgress>) {
    *progress = GameProgress::default(); // プログレス初期化
    
    // カメラ
    commands.spawn(Camera2dBundle {
        camera: Camera { clear_color: ClearColorConfig::Custom(Color::rgb(0.05, 0.05, 0.05)), ..default() },
        ..default()
    });

    // 床
    commands.spawn((
        SpriteBundle {
            sprite: Sprite { color: Color::rgb(0.2, 0.2, 0.2), custom_size: Some(Vec2::new(2000.0, 40.0)), ..default() },
            transform: Transform::from_xyz(0.0, -200.0, 0.0),
            ..default()
        },
        Collider::cuboid(1000.0, 20.0),
        Friction::coefficient(0.0),
    ));

    // プレイヤー
    commands.spawn((
        SpriteBundle {
            sprite: Sprite { color: Color::rgb(0.0, 0.9, 0.5), custom_size: Some(Vec2::new(40.0, 40.0)), ..default() },
            ..default()
        },
        Player,
        Health(3),
        RigidBody::Dynamic,
        Collider::cuboid(20.0, 20.0),
        Velocity::default(),
        ExternalImpulse::default(),
        LockedAxes::ROTATION_LOCKED,
        ActiveEvents::COLLISION_EVENTS,
    ));

    // 最初の敵 3体
    for i in 1..=3 {
        commands.spawn((
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(1.0, 0.3, 0.3), custom_size: Some(Vec2::new(40.0, 40.0)), ..default() },
                transform: Transform::from_xyz(i as f32 * 300.0, -160.0, 0.0),
                ..default()
            },
            Enemy { speed: 150.0, direction: 1.0 },
            RigidBody::KinematicVelocityBased,
            Collider::cuboid(20.0, 20.0),
            Velocity::default(),
            ActiveEvents::COLLISION_EVENTS,
        ));
    }
}

fn player_control(
    input: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut Velocity, &mut ExternalImpulse), With<Player>>,
) {
    for (mut vel, mut impulse) in query.iter_mut() {
        let mut x = 0.0;
        if input.pressed(KeyCode::ArrowLeft) { x -= 250.0; }
        if input.pressed(KeyCode::ArrowRight) { x += 250.0; }
        vel.linvel.x = x;

        if input.just_pressed(KeyCode::Space) && vel.linvel.y.abs() < 0.1 {
            impulse.impulse = Vec2::new(0.0, 50.0);
        }
        if input.just_released(KeyCode::Space) && vel.linvel.y > 0.0 {
            vel.linvel.y *= 0.4;
        }
    }
}

fn enemy_ai(mut query: Query<(&mut Enemy, &mut Velocity, &Transform), Without<Boss>>) {
    for (mut enemy, mut vel, transform) in query.iter_mut() {
        if transform.translation.x > 1000.0 { enemy.direction = -1.0; }
        else if transform.translation.x < -200.0 { enemy.direction = 1.0; }
        vel.linvel.x = enemy.direction * enemy.speed;
    }
}

fn collision_logic(
    mut commands: Commands,
    mut events: EventReader<CollisionEvent>,
    mut player_q: Query<(Entity, &Transform, &mut Health), With<Player>>,
    enemy_q: Query<(Entity, &Transform), (With<Enemy>, Without<Boss>)>,
    boss_q: Query<(Entity, &Transform), With<Boss>>,
    mut progress: ResMut<GameProgress>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    for event in events.read() {
        if let CollisionEvent::Started(e1, e2, _) = event {
            let (p_ent, p_trans, mut p_hp) = match player_q.get_single_mut() {
                Ok(q) => q,
                Err(_) => return,
            };

            let other = if *e1 == p_ent { *e2 } else { *e1 };

            // ボス判定
            if let Ok((b_ent, b_trans)) = boss_q.get(other) {
                if p_trans.translation.y > b_trans.translation.y + 30.0 {
                    // 削除の前に、まず状態を遷移させる（二重処理防止）
                    next_state.set(GameState::GameClear);
                    commands.entity(b_ent).despawn_recursive();
                    progress.boss_defeated = true;
                } else {
                    p_hp.0 = 0;
                    next_state.set(GameState::GameOver);
                }
                return; // このイベントの処理を終了
            }

            // ザコ敵判定
            if let Ok((e_ent, e_trans)) = enemy_q.get(other) {
                if p_trans.translation.y > e_trans.translation.y + 15.0 {
                    commands.entity(e_ent).despawn_recursive();
                    progress.enemies_defeated += 1;
                } else {
                    p_hp.0 -= 1;
                    if p_hp.0 <= 0 {
                        next_state.set(GameState::GameOver);
                    }
                }
            }
        }
    }
}


fn boss_system(mut commands: Commands, mut progress: ResMut<GameProgress>) {
    if progress.enemies_defeated >= 3 && !progress.boss_spawned {
        println!("*** BOSS SPAWNED ***"); // ターミナルで確認用
        commands.spawn((
            SpriteBundle {
                sprite: Sprite { color: Color::rgb(0.6, 0.0, 1.0), custom_size: Some(Vec2::new(100.0, 100.0)), ..default() },
                transform: Transform::from_xyz(400.0, -150.0, 0.0), // 少し左（プレイヤーの近く）に
                ..default()
            },
            Enemy { speed: 100.0, direction: -1.0 },
            Boss,
            RigidBody::KinematicVelocityBased,
            Collider::cuboid(50.0, 50.0),
            Velocity::default(),
            ActiveEvents::COLLISION_EVENTS, // これを忘れると踏んでも反応しません
        ));
        progress.boss_spawned = true;
    }
}

fn camera_follow(p_q: Query<&Transform, With<Player>>, mut c_q: Query<&mut Transform, (With<Camera>, Without<Player>)>) {
    if let (Ok(p), Ok(mut c)) = (p_q.get_single(), c_q.get_single_mut()) {
        c.translation.x = p.translation.x;
    }
}

fn check_end_conditions(p_q: Query<&Health, With<Player>>, mut next: ResMut<NextState<GameState>>) {
    if let Ok(hp) = p_q.get_single() {
        if hp.0 <= 0 { next.set(GameState::GameOver); }
    }
}

fn start_game(input: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if input.just_pressed(KeyCode::Space) { next.set(GameState::Playing); }
}

fn back_to_title(input: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if input.just_pressed(KeyCode::Space) { next.set(GameState::Title); }
}

fn setup_game_over(mut commands: Commands) { spawn_ui(&mut commands, "GAME OVER\n\nPress SPACE to Title", Color::RED); }
fn setup_game_clear(mut commands: Commands) { spawn_ui(&mut commands, "GAME CLEAR!\n\nPress SPACE to Title", Color::GOLD); }

// --- ユーティリティ ---

fn spawn_ui(commands: &mut Commands, txt: &str, clr: Color) {
    commands.spawn((
        NodeBundle {
            style: Style { 
                width: Val::Percent(100.0), 
                height: Val::Percent(100.0), 
                justify_content: JustifyContent::Center, 
                align_items: AlignItems::Center, 
                ..default() 
            },
            ..default()
        },
        VisualElement,
    )).with_children(|p| {
        p.spawn(TextBundle::from_section(
            txt, 
            TextStyle { 
                font_size: 50.0, 
                color: clr, 
                ..default() 
            }
        ).with_text_justify(JustifyText::Center)); // ここを修正
    });
}

fn cleanup_ui(mut commands: Commands, q: Query<Entity, With<VisualElement>>) {
    for e in q.iter() { commands.entity(e).despawn_recursive(); }
}

fn cleanup_all(
    mut commands: Commands, 
    // カメラ以外の、ゲームプレイに関連するエンティティをすべて取得
    q: Query<Entity, Or<(With<Player>, With<Enemy>, With<Collider>, With<VisualElement>)>>
) {
    for e in q.iter() {
        // すでに削除されている可能性を考慮し、存在確認なしで安全に削除予約
        commands.entity(e).despawn_recursive();
    }
}
