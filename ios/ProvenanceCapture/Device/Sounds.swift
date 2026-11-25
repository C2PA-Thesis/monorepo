import AVFoundation

/// The app's few UI sounds. Ambient, so the silent switch mutes them and
/// they never stop the person's music.
@MainActor
enum Sound: String, CaseIterable {
    case tick, success, error, shutter

    func play() {
        Self.players[self]?.currentTime = 0
        Self.players[self]?.play()
    }

    private static let players: [Sound: AVAudioPlayer] = {
        try? AVAudioSession.sharedInstance().setCategory(.ambient, options: .mixWithOthers)
        var players: [Sound: AVAudioPlayer] = [:]
        for sound in allCases {
            guard let url = Bundle.main.url(forResource: sound.rawValue, withExtension: "wav"),
                let player = try? AVAudioPlayer(contentsOf: url)
            else { continue }
            player.volume = sound.volume
            player.prepareToPlay()
            players[sound] = player
        }
        return players
    }()

    private var volume: Float {
        switch self {
        case .tick: 0.25
        case .shutter: 0.6
        case .success, .error: 0.45
        }
    }
}
