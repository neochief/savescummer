#include "demo.h"
#include "presentation.h"
#include <QDateTime>
#include <QFile>
#include <QJsonArray>
#include <QUuid>

QJsonObject demoSummary(const QJsonObject &state) {
    auto summary = state;
    QJsonObject statuses;
    const auto games = state["games"].toObject();
    for (auto it = games.begin(); it != games.end(); ++it) {
        bool any = false;
        for (const auto &snapshot : state["snapshots"].toObject())
            any |= snapshot.toObject()["game_id"] == it.key();
        const bool history = !Presentation::history(state,it.key()).isEmpty();
        statuses[it.key()] = QJsonObject{{"revision",state["revision"]}, {"has_visible_history",history}, {"can_flush",any || history}};
    }
    summary["history_status"] = statuses;
    summary.remove("history"); summary.remove("visible_history");
    return summary;
}
QJsonObject demoHistory(const QJsonObject &state, const QJsonObject &command) {
    const auto game = command["game_id"].toString();
    const auto all = Presentation::history(state,game);
    int offset = command["cursor"].toString().toInt();
    if (command["anchor_id"].isString())
        for (int i=0;i<all.size();++i) if (all[i]["id"]==command["anchor_id"]) { offset=i; break; }
    const int limit = qBound(1,command["limit"].toInt(50),200);
    const auto snapshots = state["snapshots"].toObject();
    QJsonArray rows;
    for (int i = offset; i < qMin(offset+limit,all.size()); ++i) {
        auto row = all[i];
        const auto action = Presentation::historyAction(row);
        row["action"] = action;
        row["available"] = snapshots[action["target"].toString()].toObject()["available"].toBool();
        row["display_time"] = row["kind"] == "existing_backup" ? snapshots[row["snapshot_id"].toString()].toObject()["selection_time"] : row["recorded_at"];
        for (const auto &target : state["history"].toArray())
            if (target.toObject()["id"] == row["target_id"]) row["target_time"] = target.toObject()["recorded_at"];
        rows.append(row);
    }
    return {{"type","history_page"},{"page",QJsonObject{{"game_id",game},{"revision",state["revision"]},{"rows",rows},
        {"next_cursor",offset+rows.size()<all.size() ? QJsonValue(QString::number(offset+rows.size())) : QJsonValue(QJsonValue::Null)}}}};
}

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
    emit stateChanged(demoSummary(state_));
}
void DemoService::request(const QJsonObject &command, Callback callback) {
    auto reply = QJsonObject{{"type", "ok"}};
    const auto type = command["type"].toString();
    const auto game = command["game_id"].toString();
    if (type == "history") reply = demoHistory(state_,command);
    else if (type == "state")
        reply = {{"type", "state"}, {"state", demoSummary(state_)}};
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
            } else if (type == "delete") {
                const auto target = action["target"].toString();
                snapshots.remove(target);
                QJsonArray kept;
                for (const auto &row : rows) {
                    const auto item = row.toObject();
                    if (item["snapshot_id"] != target && item["recovery_id"] != target)
                        kept.append(row);
                }
                rows = kept;
                auto a = availability[game].toObject();
                if (a["default_snapshot_id"] == target)
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
