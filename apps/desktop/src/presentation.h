#pragma once
#include <QDateTime>
#include <QJsonArray>
#include <QJsonObject>

namespace Presentation {
QString instructions(const QString &plainText);
QString age(qint64 milliseconds, const QDateTime &now = QDateTime::currentDateTime());
QString historyTime(qint64 milliseconds,
                    const QDateTime &now = QDateTime::currentDateTime());
QString day(qint64 milliseconds);
QList<QJsonObject> history(const QJsonObject &state, const QString &game);
QJsonObject blockingOperation(const QJsonObject &state, const QString &game);
QJsonObject historyAction(const QJsonObject &row);
QStringList orderedGames(const QJsonObject &state);
} // namespace Presentation
