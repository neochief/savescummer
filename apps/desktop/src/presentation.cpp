#include "presentation.h"
#include <QLocale>
#include <QRegularExpression>
#include <algorithm>

QString Presentation::instructions(const QString &text) {
    if (text.trimmed().isEmpty())
        return "<p>No instructions available for this game yet.</p>";
    QString html;
    bool list = false;
    const QRegularExpression numbered("^\\d+\\.\\s+(.+)$");
    for (const auto &raw : text.split('\n')) {
        const auto line = raw.trimmed();
        const auto match = numbered.match(line);
        if (match.hasMatch()) {
            if (!list) {
                html += "<ol style='margin-top:4px; margin-bottom:10px; margin-left:18px; "
                        "-qt-list-indent:0;'>";
                list = true;
            }
            html += "<li style='margin-bottom:4px;'>" + match.captured(1).toHtmlEscaped() + "</li>";
        } else {
            if (list) {
                html += "</ol>";
                list = false;
            }
            if (!line.isEmpty()) {
                const auto escaped = line.toHtmlEscaped();
                html += "<p style='margin-top:4px; margin-bottom:4px;'>" +
                        (line.endsWith(':') ? "<b>" + escaped + "</b>" : escaped) + "</p>";
            }
        }
    }
    if (list)
        html += "</ol>";
    return html;
}
QString Presentation::age(qint64 milliseconds, const QDateTime &now) {
    const auto date = QDateTime::fromMSecsSinceEpoch(milliseconds).toLocalTime();
    const auto seconds = qMax<qint64>(0, date.secsTo(now));
    if (seconds < 60)
        return seconds <= 1 ? "just now" : QString("%1 seconds ago").arg(seconds);
    if (seconds < 3600)
        return QString("%1 minute%2 ago").arg(seconds / 60).arg(seconds / 60 == 1 ? "" : "s");
    if (date.date() == now.date()) {
        const auto hours = seconds / 3600, minutes = seconds % 3600 / 60;
        return QString("%1 hour%2%3 ago")
            .arg(hours)
            .arg(hours == 1 ? "" : "s")
            .arg(minutes ? QString(" and %1 minute%2").arg(minutes).arg(minutes == 1 ? "" : "s")
                         : "");
    }
    if (date.date() == now.date().addDays(-1))
        return "yesterday, " + date.toString("HH:mm:ss");
    if (date.daysTo(now) < 7)
        return QLocale().toString(date, "dddd, HH:mm:ss");
    return date.toString("yyyy-MM-dd, HH:mm:ss");
}
QString Presentation::historyTime(qint64 milliseconds, const QDateTime &now) {
    const auto date = QDateTime::fromMSecsSinceEpoch(milliseconds).toLocalTime();
    const auto days = date.date().daysTo(now.date());
    const auto day = days == 0       ? QString("Today")
                     : days == 1     ? QString("Yesterday")
                     : days < 7 && days > 1
                         ? QLocale().toString(date.date(), "dddd")
                         : date.toString("yyyy-MM-dd");
    return day + "\n" + date.toString("HH:mm:ss");
}
QString Presentation::day(qint64 milliseconds) {
    const auto date = QDateTime::fromMSecsSinceEpoch(milliseconds).toLocalTime().date();
    if (date == QDate::currentDate())
        return "Today";
    if (date == QDate::currentDate().addDays(-1))
        return "Yesterday";
    return QLocale().toString(date, QLocale::LongFormat);
}
QList<QJsonObject> Presentation::history(const QJsonObject &state, const QString &game) {
    QList<QJsonObject> rows;
    for (const auto &value : state["visible_history"].toArray())
        if (value.toObject()["game_id"] == game)
            rows.append(value.toObject());
    std::sort(rows.begin(), rows.end(), [](const auto &a, const auto &b) {
        return a["sequence"].toInteger() > b["sequence"].toInteger();
    });
    return rows;
}
QJsonObject Presentation::blockingOperation(const QJsonObject &state, const QString &game) {
    const auto operations = state["operations"].toObject();
    QJsonObject recovery;
    for (const auto &value : operations) {
        const auto op = value.toObject();
        if (op["game_id"] != game)
            continue;
        if (op["status"] == "pending")
            return op;
        if (op["status"] == "recovery_needed")
            recovery = op;
    }
    return recovery;
}
QJsonObject Presentation::historyAction(const QJsonObject &row) {
    const auto kind = row["kind"].toString();
    if ((kind == "saved" || kind == "existing_backup") && row["snapshot_id"].isString())
        return {{"type", "load"}, {"target", row["snapshot_id"]}};
    if ((kind == "loaded" || kind == "reverted") && row["recovery_id"].isString())
        return {{"type", "revert"}, {"target", row["recovery_id"]}};
    return {};
}
QStringList Presentation::orderedGames(const QJsonObject &state) {
    const auto games = state["games"].toObject();
    QStringList result;
    for (const auto &id : state["active_stack"].toArray())
        if ((games[id.toString()].toObject()["installed"].toBool() ||
             games[id.toString()].toObject()["origin"] == "custom") &&
            !result.contains(id.toString()))
            result.append(id.toString());
    QStringList rest;
    for (auto it = games.begin(); it != games.end(); ++it)
        if ((it.value().toObject()["installed"].toBool() ||
             it.value().toObject()["origin"] == "custom") &&
            !result.contains(it.key()))
            rest.append(it.key());
    const auto availability = state["availability"].toObject();
    std::sort(rest.begin(), rest.end(), [&games, &availability](const auto &a, const auto &b) {
        const bool aReady = availability[a].toObject()["data_available"].toBool();
        const bool bReady = availability[b].toObject()["data_available"].toBool();
        if (aReady != bReady)
            return aReady;
        return QString::localeAwareCompare(games[a].toObject()["name"].toString(),
                                           games[b].toObject()["name"].toString()) < 0;
    });
    return result + rest;
}
