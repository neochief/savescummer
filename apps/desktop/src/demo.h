#pragma once
#include "service.h"
QJsonObject demoState();
QJsonObject demoSummary(const QJsonObject &state);
QJsonObject demoHistory(const QJsonObject &state, const QJsonObject &command);
class DemoService final : public Service {
    Q_OBJECT
  public:
    using Service::Service;
    void start() override;
    void request(const QJsonObject &command, Callback callback = {}) override;

  private:
    void publish();
    QJsonObject state_ = demoState();
};
