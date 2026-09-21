#include "service.h"
#include <QDir>
#include <QJsonDocument>
#include <QPointer>
#include <QProcess>
#include <QUuid>
#include <QtEndian>
#include <stdexcept>
#ifdef Q_OS_WIN
#define NOMINMAX
#include <windows.h>
#include <sddl.h>
#endif

namespace {
QJsonObject failure(const QString &message) {
    return {{"type", "error"},
            {"error", QJsonObject{{"code", "connection"}, {"message", message}}}};
}
} // namespace
QByteArray Wire::frame(const QJsonObject &object) {
    const auto json = QJsonDocument(object).toJson(QJsonDocument::Compact);
    if (json.isEmpty() || json.size() > MaxFrame)
        throw std::runtime_error("Invalid message size");
    QByteArray result(4, '\0');
    qToLittleEndian<quint32>(quint32(json.size()), result.data());
    return result + json;
}
QList<QJsonObject> Wire::consume(QByteArray &buffer) {
    QList<QJsonObject> result;
    while (buffer.size() >= 4) {
        const auto size = qFromLittleEndian<quint32>(buffer.constData());
        if (!size || size > MaxFrame)
            throw std::runtime_error("Invalid service frame size");
        if (buffer.size() < qint64(size) + 4)
            break;
        QJsonParseError error;
        const auto document = QJsonDocument::fromJson(buffer.mid(4, size), &error);
        if (error.error != QJsonParseError::NoError || !document.isObject())
            throw std::runtime_error("Invalid service JSON");
        result.append(document.object());
        buffer.remove(0, size + 4);
    }
    return result;
}
QJsonObject Wire::envelope(const QJsonObject &command, const QString &id) {
    return {{"version", Version}, {"request_id", id}, {"command", command}};
}
QString Wire::endpoint(const QString &directory) {
#ifdef Q_OS_WIN
    const auto native = QDir::toNativeSeparators(QDir(directory).absolutePath());
    HANDLE file = CreateFileW(reinterpret_cast<LPCWSTR>(native.utf16()), 0,
                              FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                              OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr);
    if (file == INVALID_HANDLE_VALUE)
        return {};
    std::wstring path(32768, L'\0');
    DWORD size =
        GetFinalPathNameByHandleW(file, path.data(), DWORD(path.size()), FILE_NAME_NORMALIZED);
    CloseHandle(file);
    if (!size || size >= path.size())
        return {};
    const auto bytes = QString::fromStdWString(path.substr(0, size)).toLower().toUtf8();
    quint64 hash = 0xcbf29ce484222325ULL;
    for (unsigned char byte : bytes)
        hash = (hash ^ byte) * 0x100000001b3ULL;
    HANDLE token;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token))
        return {};
    DWORD length = 0;
    GetTokenInformation(token, TokenUser, nullptr, 0, &length);
    QByteArray info(int(length), '\0');
    bool ok = GetTokenInformation(token, TokenUser, info.data(), length, &length);
    CloseHandle(token);
    LPWSTR sid = nullptr;
    if (!ok || !ConvertSidToStringSidW(reinterpret_cast<TOKEN_USER *>(info.data())->User.Sid, &sid))
        return {};
    const QString user = QString::fromWCharArray(sid);
    LocalFree(sid);
    return QStringLiteral("savescummer-v4-%1-%2").arg(user, QString::number(hash, 16));
#else
    return QDir(directory).absoluteFilePath("host.sock");
#endif
}
LocalService::LocalService(QString endpoint, QString host, QStringList arguments, QObject *parent)
    : Service(parent), endpoint_(std::move(endpoint)), host_(std::move(host)),
      hostArguments_(std::move(arguments)) {
    retry_.setInterval(1500);
    retry_.setSingleShot(true);
    connect(&retry_, &QTimer::timeout, this, &LocalService::watch);
}
void LocalService::start() {
    watch();
}
void LocalService::disconnected(const QString &message) {
    if (stopping_) return;
    connected_ = false;
    emit connectionChanged(false, message);
    if (!startedHost_ && !host_.isEmpty()) {
        startedHost_ = true;
        QProcess process;
        process.setProgram(host_);
        process.setArguments(hostArguments_);
#ifdef Q_OS_WIN
        process.setCreateProcessArgumentsModifier(
            [](QProcess::CreateProcessArguments *args) { args->flags |= CREATE_NO_WINDOW; });
#endif
        if (!process.startDetached())
            emit connectionChanged(false,
                                   "Cannot start the background host. Check the --host path.");
    }
    retry_.start();
}
void LocalService::watch() {
    if (watcher_) {
        watcher_->disconnect(this);
        watcher_->abort();
        watcher_->deleteLater();
    }
    auto *socket = new QLocalSocket(this);
    watcher_ = socket;
    const QString id = QUuid::createUuid().toString(QUuid::WithoutBraces);
    auto buffer = std::make_shared<QByteArray>();
    connect(socket, &QLocalSocket::connected, this, [socket, id] {
        socket->write(Wire::frame(Wire::envelope({{"type", "watch"}}, id)));
    });
    connect(socket, &QLocalSocket::readyRead, this, [this, socket, id, buffer] {
        buffer->append(socket->readAll());
        try {
            for (const auto &response : Wire::consume(*buffer)) {
                if (response["version"].toInt() == Wire::Version && response["request_id"].toString() == id &&
                    response["result"].toObject()["type"] == "shutting_down") {
                    stopping_ = true;
                    retry_.stop();
                    emit hostStopping();
                    return;
                }
                if (response["version"].toInt() != Wire::Version ||
                    response["request_id"].toString() != id ||
                    response["host_id"].toString().isEmpty() ||
                    response["result"].toObject()["type"] != "state")
                    throw std::runtime_error("Incompatible background host response");
                const auto state = response["result"].toObject()["state"].toObject();
                const auto host = response["host_id"].toString();
                const bool reconnected = !connected_;
                if (host != hostId_) {
                    hostId_ = host;
                    revision_ = -1;
                    artworkRevision_ = -1;
                    scanInProgress_ = false;
                }
                connected_ = true;
                emit connectionChanged(true, {});
                const auto revision = state["revision"].toInteger();
                const auto artworkRevision = state["artwork_revision"].toInteger();
                const auto scanInProgress = state["scan_in_progress"].toBool();
                if (reconnected || revision > revision_ || artworkRevision > artworkRevision_ ||
                    scanInProgress != scanInProgress_) {
                    revision_ = revision;
                    artworkRevision_ = artworkRevision;
                    scanInProgress_ = scanInProgress;
                    emit stateChanged(state);
                }
                const auto operations = state["operations"].toObject();
                for (auto it = accepted_.begin(); it != accepted_.end();) {
                    const auto op = operations[*it].toObject();
                    if (!op.isEmpty() && op["status"] != "pending")
                        it = accepted_.erase(it);
                    else
                        ++it;
                }
                if (reconnected) {
                    request({{"type", "check_artwork"}});
                    // Never replay a destructive request after reconnecting.
                    for (const auto &operation : std::as_const(accepted_))
                        request({{"type", "operation"}, {"operation_id", operation}});
                }
            }
        } catch (const std::exception &e) {
            socket->abort();
            disconnected(QString::fromUtf8(e.what()));
        }
    });
    connect(socket, &QLocalSocket::disconnected, this, [this] {
        disconnected("Disconnected from the background host. Reconnecting… Operations may still be "
                     "running.");
    });
    connect(socket, &QLocalSocket::errorOccurred, this,
            [this](auto) { disconnected("Waiting for the background host…"); });
    socket->connectToServer(endpoint_);
}
void LocalService::request(const QJsonObject &command, Callback callback) {
    auto *socket = new QLocalSocket(this);
    auto *timeout = new QTimer(socket);
    timeout->setSingleShot(true);
    const auto id = QUuid::createUuid().toString(QUuid::WithoutBraces);
    auto done = std::make_shared<bool>(false);
    auto buffer = std::make_shared<QByteArray>();
    const auto finish = [socket, done, callback](const QJsonObject &reply) {
        if (*done)
            return;
        *done = true;
        if (callback)
            callback(reply);
        socket->abort();
        socket->deleteLater();
    };
    connect(socket, &QLocalSocket::connected, this,
            [socket, command, id] { socket->write(Wire::frame(Wire::envelope(command, id))); });
    connect(socket, &QLocalSocket::readyRead, this, [this, socket, id, buffer, finish] {
        buffer->append(socket->readAll());
        try {
            const auto frames = Wire::consume(*buffer);
            if (frames.isEmpty())
                return;
            const auto response = frames.first();
            if (response["version"].toInt() != Wire::Version ||
                response["request_id"].toString() != id ||
                response["host_id"].toString().isEmpty() || !response["result"].isObject())
                throw std::runtime_error("Incompatible background host response");
            const auto reply = response["result"].toObject();
            if (reply["type"] == "accepted")
                accepted_.insert(reply["operation_id"].toString());
            if (reply["type"] == "operation" &&
                reply["operation"].toObject()["status"] != "pending")
                accepted_.remove(reply["operation"].toObject()["id"].toString());
            finish(reply);
        } catch (const std::exception &e) {
            finish(failure(QString::fromUtf8(e.what())));
        }
    });
    const auto uncertain = [finish, id] {
        finish(failure("Connection lost; the outcome is unknown. Reconnect to inspect current "
                       "state. Request: " +
                       id));
    };
    connect(socket, &QLocalSocket::errorOccurred, this, [uncertain](auto) { uncertain(); });
    connect(socket, &QLocalSocket::disconnected, this, uncertain);
    connect(timeout, &QTimer::timeout, this, uncertain);
    timeout->start(15000);
    socket->connectToServer(endpoint_);
}
