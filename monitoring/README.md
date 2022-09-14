## Prometheus Monitoring 

Prometheus is a monitoring platform that collects metrics from monitored targets by scraping metrics HTTP endpoints on these targets. 

## Prerequisites

First install and configure prometheus. You can download specific versions by visiting the prometheus website [here](https://prometheus.io/docs/prometheus/latest/installation/). If you 
are using a Mac you can install via brew:
```
brew install prometheus
```

Wants installed you will need to create a config file to inform prometheus what to scrape. The configuration you may use out of the box is included in this directory (e.g. `prometheus-config.yaml`). 


## Usage

Start substrate node
```
./scripts/run-standalone.sh 
```

Start prometheus client from monitoring directory 
```
prometheus --config.file="prometheus-config.yaml"
```

Navigate to `http://localhost:9090/graph` or to see all the scraped metrics `http://localhost:9090/metrics`

