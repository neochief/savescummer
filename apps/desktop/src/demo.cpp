#include "demo.h"
#include "presentation.h"
#include <QDateTime>
#include <QFile>
#include <QJsonArray>
#include <QUuid>

QJsonObject demoState() {
    QFile catalog(":/void-war.yaml");
    catalog.open(QIODevice::ReadOnly);
    const auto lines = QString::fromUtf8(catalog.readAll()).split('\n');
    QString info;
    bool inInfo = false;
    for (const auto &line : lines) {
        if (line.startsWith("info: |")) {
            inInfo = true;
            continue;
        }
        if (!inInfo)
            continue;
        if (!line.isEmpty() && !line.startsWith("  "))
            break;
        info += line.mid(2) + '\n';
    }
    QJsonObject games, availability, snapshots;
    QJsonArray history;
    const QList<QPair<QString, QString>> names{{"void-war", "Void War"},
                                               {"ftl", "FTL: Faster Than Light"},
                                               {"slay-the-spire", "Slay the Spire"},
                                               {"into-the-breach", "Into the Breach"}};
    qint64 sequence = 1;
    for (const auto &pair : names) {
        const auto id = pair.first;
        games[id] = QJsonObject{{"id", id},
                                {"name", pair.second},
                                {"info", id == "void-war" ? info : QString()},
                                {"installed", true},
                                {"data_dir", ""},
                                {"executables", QJsonArray()},
                                {"configuration_error", QJsonValue::Null}};
        const bool data = id != "into-the-breach";
        availability[id] = QJsonObject{
            {"data_available", data},
            {"default_snapshot_id", data ? QJsonValue(id + "-saved") : QJsonValue::Null}};
        if (data) {
            const auto time = QDateTime::currentMSecsSinceEpoch() - (id == "void-war" ? 120000
                                                                     : id == "ftl"    ? 86400000
                                                                                   : 2 * 86400000);
            snapshots[id + "-saved"] = QJsonObject{{"id", id + "-saved"},
                                                   {"game_id", id},
                                                   {"kind", "saved"},
                                                   {"available", true},
                                                   {"saved_at", time},
                                                   {"selection_time", time},
                                                   {"removed_at", QJsonValue::Null}};
            history.append(QJsonObject{{"id", id + "-history"},
                                       {"game_id", id},
                                       {"kind", "saved"},
                                       {"sequence", sequence++},
                                       {"recorded_at", time},
                                       {"snapshot_id", id + "-saved"}});
        }
    }
    return {{"revision", 1},
            {"settings", QJsonObject{{"play_sounds", true}}},
            {"games", games},
            {"availability", availability},
            {"snapshots", snapshots},
            {"active_stack", QJsonArray{"void-war"}},
            {"history", history},
            {"visible_history", history},
            {"operations", QJsonObject()}};
}
void DemoService::start() {
    emit connectionChanged(true, {});
    publish();
}
void DemoService::publish() {
    state_["revision"] = state_["revision"].toInteger() + 1;
    emit stateChanged(state_);
}
void DemoService::request(const QJsonObject &command, Callback callback) {
    auto reply = QJsonObject{{"type", "ok"}};
    const auto type = command["type"].toString();
    const auto game = command["game_id"].toString();
    if (type == "state" || type == "history")
        reply = {{"type", "state"}, {"state", state_}};
    else if (type == "set_play_sounds" || type == "set_launch_on_startup") {
        auto settings = state_["settings"].toObject();
        settings[type == "set_play_sounds" ? "play_sounds" : "launch_on_startup"] = command["enabled"].toBool();
        state_["settings"] = settings;
        publish();
    } else if (type == "configure") {
        auto games = state_["games"].toObject();
        auto item = games[command["id"].toString()].toObject();
        item["data_dir"] = command["data_dir"];
        item["executables"] = command["executables"];
        games[command["id"].toString()] = item;
        state_["games"] = games;
        publish();
        reply = {{"type", "configured"}, {"game", item}};
    } else if (type == "flush_preview") {
        int saved = 0, recovery = 0;
        for (const auto &snapshot : state_["snapshots"].toObject())
            if (snapshot.toObject()["game_id"] == game) {
                if (snapshot.toObject()["kind"] == "saved")
                    ++saved;
                else
                    ++recovery;
            }
        reply = {{"type", "flush_preview"},
                 {"preview", QJsonObject{{"revision", state_["revision"]},
                                         {"saved", saved},
                                         {"recovery", recovery},
                                         {"retained", 0},
                                         {"paths", QJsonArray()}}}};
    } else if (type == "execute") {
        const auto action = command["action"].toObject();
        const auto id = QUuid::createUuid().toString(QUuid::WithoutBraces);
        auto operations = state_["operations"].toObject();
        operations[id] = QJsonObject{{"id", id},
                                     {"game_id", game},
                                     {"status", "pending"},
                                     {"action", action},
                                     {"phase", "preparing"},
                                     {"bytes_copied", 0},
                                     {"started_at", QDateTime::currentMSecsSinceEpoch()}};
        state_["operations"] = operations;
        publish();
        reply = {{"type", "accepted"}, {"operation_id", id}};
        QTimer::singleShot(900, this, [this, id, game, action] {
            auto operations = state_["operations"].toObject();
            auto op = operations[id].toObject();
            op["status"] = "completed";
            operations[id] = op;
            state_["operations"] = operations;
            auto snapshots = state_["snapshots"].toObject();
            auto rows = state_["visible_history"].toArray();
            auto availability = state_["availability"].toObject();
            const auto type = action["type"].toString();
            if (type == "flush") {
                for (auto it = snapshots.begin(); it != snapshots.end();)
                    if (it.value().toObject()["game_id"] == game)
                        it = snapshots.erase(it);
                    else
                        ++it;
                QJsonArray kept;
                for (const auto &row : rows)
                    if (row.toObject()["game_id"] != game)
                        kept.append(row);
                rows = kept;
                auto a = availability[game].toObject();
                a["default_snapshot_id"] = QJsonValue::Null;
                availability[game] = a;
            } else {
                const bool save = type == "save";
                const auto time = QDateTime::currentMSecsSinceEpoch();
                snapshots[id] = QJsonObject{
                    {"id", id},          {"game_id", game},  {"kind", save ? "saved" : "recovery"},
                    {"available", true}, {"saved_at", time}, {"removed_at", QJsonValue::Null}};
                qint64 sequence = 0;
                for (const auto &row : rows)
                    sequence = qMax(sequence, row.toObject()["sequence"].toInteger());
                rows.append(QJsonObject{{"id", id + "-history"},
                                        {"game_id", game},
                                        {"kind", save               ? "saved"
                                                 : type == "revert" ? "reverted"
                                                                    : "loaded"},
                                        {"sequence", sequence + 1},
                                        {"recorded_at", time},
                                        {save ? "snapshot_id" : "recovery_id", id}});
                if (save) {
                    auto a = availability[game].toObject();
                    a["default_snapshot_id"] = id;
                    availability[game] = a;
                }
            }
            state_["snapshots"] = snapshots;
            state_["history"] = rows;
            state_["visible_history"] = rows;
            state_["availability"] = availability;
            publish();
        });
    }
    if (callback)
        QTimer::singleShot(0, this, [callback, reply] { callback(reply); });
}
