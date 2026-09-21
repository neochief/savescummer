#pragma once
#include <QJsonObject>
#include <QLocalSocket>
#include <QPointer>
#include <QSet>
#include <QTimer>
#include <functional>
#include <memory>

// UI depends only on this interface. The demo and tests never touch game files.
class Service : public QObject {
    Q_OBJECT
  public:
    using Callback = std::function<void(const QJsonObject &)>;
    using QObject::QObject;
    virtual void start() = 0;
    virtual void request(const QJsonObject &command, Callback callback = {}) = 0;
  signals:
    void stateChanged(const QJsonObject &state);
    void connectionChanged(bool connected, const QString &explanation);
    void hostStopping();
};

namespace Wire {
constexpr int Version = 4;
constexpr quint32 MaxFrame = 8 * 1024 * 1024;
QByteArray frame(const QJsonObject &object);
// Consumes complete frames, retaining a partial tail. Throws on invalid framing/JSON.
QList<QJsonObject> consume(QByteArray &buffer);
QJsonObject envelope(const QJsonObject &command, const QString &id);
QString endpoint(const QString &dataDirectory);
} // namespace Wire

class LocalService final : public Service {
    Q_OBJECT
  public:
    LocalService(QString endpoint, QString host, QStringList hostArguments,
                 QObject *parent = nullptr);
    void start() override;
    void request(const QJsonObject &command, Callback callback = {}) override;

  private:
    void watch();
    void disconnected(const QString &message);
    QString endpoint_, host_, hostId_;
    QStringList hostArguments_;
    QTimer retry_;
    QPointer<QLocalSocket> watcher_;
    QSet<QString> accepted_;
    qint64 revision_ = -1;
    qint64 artworkRevision_ = -1;
    bool scanInProgress_ = false;
    bool startedHost_ = false;
    bool connected_ = false;
    bool stopping_ = false;
};
