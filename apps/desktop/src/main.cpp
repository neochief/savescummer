#include "demo.h"
#include "mainwindow.h"
#include <QApplication>
#include <QCommandLineParser>
#include <QDir>
#include <QFileInfo>
#include <QMessageBox>
#include <QSettings>
#include <QStandardPaths>
#include <QStyleHints>
#include <QLocalServer>
#include <QLockFile>
#include <QCryptographicHash>

int main(int argc, char **argv) {
    QApplication app(argc, argv);
    app.setApplicationName("SaveScummer");
    app.setOrganizationName("SaveScummer");
    app.setStyle("Fusion");
    QCommandLineParser parser;
    parser.setApplicationDescription("Save Scummer Qt desktop client");
    parser.addHelpOption();
    parser.addOption({"minimized", "Start the host without showing the main window."});
    parser.addOption(
        {"demo", "Run an isolated demonstration. No game files or system settings are changed."});
    parser.addOption(
        {"data-dir", "Host data directory (for isolated development instances).", "directory"});
    parser.addOption({"host", "Background host executable.", "executable"});
    parser.addOption(
        {"endpoint", "Connect to an explicit local endpoint without starting a host.", "name"});
    parser.addOption({"theme", "Appearance: system, light or dark.", "theme", "system"});
    parser.process(app);
    const auto theme = parser.value("theme");
    MainWindow::applyTheme(
        theme == "dark" ||
        (theme != "light" && app.styleHints()->colorScheme() == Qt::ColorScheme::Dark));
    if (theme == "system")
        QObject::connect(app.styleHints(), &QStyleHints::colorSchemeChanged, &app, [](auto scheme) {
            MainWindow::applyTheme(scheme == Qt::ColorScheme::Dark);
        });
    std::unique_ptr<Service> service;
    QString activationEndpoint;
    const bool demo = parser.isSet("demo");
    if (demo)
        service = std::make_unique<DemoService>();
    else {
        QString endpoint = parser.value("endpoint"), host = parser.value("host");
        QStringList args;
        if (endpoint.isEmpty()) {
            QString directory = parser.value("data-dir");
            if (directory.isEmpty()) {
#ifdef Q_OS_WIN
                directory = qEnvironmentVariable("LOCALAPPDATA") + "/SaveScummer";
#else
                directory = QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation) +
                            "/SaveScummer";
#endif
            }
            directory = QDir(directory).absolutePath();
            if (!QDir().mkpath(directory)) {
                QMessageBox::critical(nullptr, "Save Scummer",
                                      "Cannot create the application data directory.");
                return 1;
            }
            endpoint = Wire::endpoint(directory);
            if (endpoint.isEmpty()) {
                QMessageBox::critical(nullptr, "Save Scummer",
                                      "Cannot resolve the local host endpoint.");
                return 1;
            }
            args = {"--data-dir", directory, "--minimized", "--desktop", app.applicationFilePath()};
            if (host.isEmpty()) {
#ifdef Q_OS_WIN
                const QString filename = "savescummer-host.exe";
#else
                const QString filename = "savescummer-host";
#endif
                host = QDir(app.applicationDirPath()).filePath(filename);
                if (!QFileInfo::exists(host))
                    host = QStandardPaths::findExecutable(filename);
            }
        }
        activationEndpoint = endpoint + "-desktop";
        service = std::make_unique<LocalService>(endpoint, host, args);
    }
    QLocalServer activation;
    std::unique_ptr<QLockFile> activationLock;
    if (!demo) {
        QLocalSocket existing;
        existing.connectToServer(activationEndpoint);
        if (existing.waitForConnected(500)) {
            if (!parser.isSet("minimized")) { existing.write("s"); existing.waitForBytesWritten(500); }
            return 0;
        }
        const auto hash = QCryptographicHash::hash(activationEndpoint.toUtf8(), QCryptographicHash::Sha256).toHex();
        activationLock = std::make_unique<QLockFile>(QDir::temp().filePath("savescummer-ui-" + hash + ".lock"));
        if (!activationLock->tryLock(1000)) return 0;
        QLocalServer::removeServer(activationEndpoint);
        activation.setSocketOptions(QLocalServer::UserAccessOption);
        if (!activation.listen(activationEndpoint)) return 1;
    }
    MainWindow window(service.get(), demo);
    QObject::connect(&activation, &QLocalServer::newConnection, &window, [&] {
        while (auto *socket = activation.nextPendingConnection()) {
            auto show = [socket, &window] {
                if (!socket->readAll().isEmpty()) { window.showNormal(); window.raise(); window.activateWindow(); }
            };
            QObject::connect(socket, &QLocalSocket::readyRead, &window, show);
            if (socket->bytesAvailable()) show();
            QObject::connect(socket, &QLocalSocket::disconnected, socket, &QObject::deleteLater);
            QTimer::singleShot(1000, socket, &QObject::deleteLater);
        }
    });
    QObject::connect(service.get(), &Service::hostStopping, &app, &QApplication::quit);
    QSettings settings;
    if (!demo)
        window.restoreGeometry(settings.value("desktop/geometry").toByteArray());
    if (!parser.isSet("minimized")) window.show();
    service->start();
    QObject::connect(&app, &QApplication::aboutToQuit, &window, [&] {
        if (!demo)
            settings.setValue("desktop/geometry", window.saveGeometry());
    });
    return app.exec();
}
